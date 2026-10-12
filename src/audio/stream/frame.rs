//! 码流分帧：从网络字节流里切出完整的 AAC（ADTS）/ MP3 帧。中途接入的流开头可能是半帧或 ID3 标签，
//! 连续两个帧头对得上才算同步；之后每帧都检查帧头，对不上就重新找。

/// 一帧的格式（同一路流里应保持不变）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Aac,
    Mp3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: Kind,
    pub rate: u32,
    pub channels: u16,
    /// 帧长（字节，含帧头）
    pub len: usize,
    /// MP3 的码率（kbps）；AAC 为 0
    pub kbps: u32,
}

impl Header {
    /// 同一路流的帧：格式、采样率、声道相同
    fn same_stream(&self, o: &Header) -> bool {
        self.kind == o.kind && self.rate == o.rate && self.channels == o.channels
    }
}

const AAC_RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

/// ADTS 帧头（7 字节起）
pub fn adts(b: &[u8]) -> Option<Header> {
    if b.len() < 7 || b[0] != 0xFF || b[1] & 0xF6 != 0xF0 {
        return None;
    }
    let sr = ((b[2] >> 2) & 0x0F) as usize;
    let ch = ((b[2] & 1) << 2) | (b[3] >> 6);
    let len = (((b[3] & 3) as usize) << 11) | ((b[4] as usize) << 3) | (b[5] >> 5) as usize;
    if sr >= AAC_RATES.len() || ch == 0 || len < 7 {
        return None;
    }
    Some(Header { kind: Kind::Aac, rate: AAC_RATES[sr], channels: if ch == 7 { 8 } else { ch as u16 }, len, kbps: 0 })
}

/// MPEG 音频（MP1 / MP2 / MP3）帧头（4 字节）
pub fn mpa(b: &[u8]) -> Option<Header> {
    if b.len() < 4 || b[0] != 0xFF || b[1] & 0xE0 != 0xE0 {
        return None;
    }
    let ver = (b[1] >> 3) & 3; // 0 = 2.5，2 = 2，3 = 1
    let layer = (b[1] >> 1) & 3; // 1 = III，2 = II，3 = I
    let br = (b[2] >> 4) as usize;
    let sr = ((b[2] >> 2) & 3) as usize;
    if ver == 1 || layer == 0 || br == 0 || br == 15 || sr == 3 {
        return None;
    }
    let pad = ((b[2] >> 1) & 1) as usize;
    let v1 = ver == 3;
    const KBPS: [[[u16; 15]; 3]; 2] = [
        // MPEG-1：Layer I、II、III
        [
            [0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448],
            [0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384],
            [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320],
        ],
        // MPEG-2 / 2.5：Layer I、II / III
        [
            [0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256],
            [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
            [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
        ],
    ];
    let li = (3 - layer) as usize; // 0 = I，1 = II，2 = III
    let kbps = KBPS[if v1 { 0 } else { 1 }][li][br] as u32;
    let rate = [44100, 48000, 32000][sr] >> match ver {
        3 => 0,
        2 => 1,
        _ => 2,
    };
    let len = match li {
        0 => (12 * kbps as usize * 1000 / rate as usize + pad) * 4,
        1 => 144 * kbps as usize * 1000 / rate as usize + pad,
        _ => (if v1 { 144 } else { 72 }) * kbps as usize * 1000 / rate as usize + pad,
    };
    let channels = if b[3] >> 6 == 3 { 1 } else { 2 };
    (len >= 4).then_some(Header { kind: Kind::Mp3, rate, channels, len, kbps })
}

fn header(kind: Option<Kind>, b: &[u8]) -> Option<Header> {
    match kind {
        Some(Kind::Aac) => adts(b),
        Some(Kind::Mp3) => mpa(b),
        None => adts(b).or_else(|| mpa(b)),
    }
}

/// 把陆续收到的字节切成帧
#[derive(Default)]
pub struct Framer {
    buf: Vec<u8>,
    pos: usize,
    /// 已同步的流（之后的帧必须与它一致）
    locked: Option<Header>,
    /// 只接受这种格式（例如 HLS 里已知是 AAC）
    pub only: Option<Kind>,
}

impl Framer {
    pub fn push(&mut self, data: &[u8]) {
        if self.pos > 0 && self.pos * 2 > self.buf.len() {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.extend_from_slice(data);
    }

    pub fn locked(&self) -> Option<Header> {
        self.locked
    }

    /// 下一帧（帧头 + 整帧数据）；数据不够时返回 None
    pub fn next(&mut self) -> Option<(Header, Vec<u8>)> {
        loop {
            let rest = &self.buf[self.pos..];
            if rest.len() < 8 {
                return None;
            }
            let kind = self.only.or(self.locked.map(|h| h.kind));
            let h = match header(kind, rest) {
                Some(h) if self.locked.is_none_or(|l| l.same_stream(&h)) => h,
                _ => {
                    self.resync();
                    continue;
                }
            };
            if rest.len() < h.len + 8 && self.locked.is_none() {
                // 还没同步：要看到下一帧的帧头才确认
                return None;
            }
            if self.locked.is_none() {
                match header(Some(h.kind), &rest[h.len..]) {
                    Some(n) if n.same_stream(&h) => self.locked = Some(h),
                    _ => {
                        self.pos += 1;
                        continue;
                    }
                }
            }
            if rest.len() < h.len {
                return None;
            }
            let f = rest[..h.len].to_vec();
            self.pos += h.len;
            return Some((h, f));
        }
    }

    /// 同步丢了：往后找下一个可能的帧头（跳过 ID3 标签）
    fn resync(&mut self) {
        let rest = &self.buf[self.pos..];
        if rest.len() >= 10 && &rest[..3] == b"ID3" {
            let size = rest[6..10].iter().fold(0usize, |a, &b| (a << 7) | (b & 0x7F) as usize);
            self.pos += (10 + size).min(rest.len());
            return;
        }
        self.locked = None;
        self.pos += 1 + rest[1..].iter().position(|&b| b == 0xFF).unwrap_or(rest.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adts_frame(len: usize) -> Vec<u8> {
        // AAC LC，44100 Hz（索引 4），2 声道
        let mut v = vec![0xFF, 0xF1, (1 << 6) | (4 << 2), (2 << 6) | ((len >> 11) & 3) as u8, (len >> 3) as u8, ((len & 7) << 5) as u8 | 0x1F, 0xFC];
        v.resize(len, 0x55);
        v
    }

    #[test]
    fn headers() {
        let h = adts(&adts_frame(300)).unwrap();
        assert_eq!((h.kind, h.rate, h.channels, h.len), (Kind::Aac, 44100, 2, 300));
        // MPEG-1 Layer III 128 kbps 44100 Hz 立体声：417 字节（不加填充）
        let h = mpa(&[0xFF, 0xFB, 0x90, 0x00]).unwrap();
        assert_eq!((h.kind, h.rate, h.channels, h.len, h.kbps), (Kind::Mp3, 44100, 2, 417, 128));
        // 填充位、单声道
        assert_eq!(mpa(&[0xFF, 0xFB, 0x92, 0xC0]).map(|h| (h.len, h.channels)), Some((418, 1)));
        // MPEG-2 Layer III 64 kbps 22050 Hz：72 * 64000 / 22050 = 208
        assert_eq!(mpa(&[0xFF, 0xF3, 0x80, 0x00]).map(|h| (h.rate, h.len)), Some((22050, 208)));
        assert_eq!(mpa(&[0xFF, 0xFB, 0xF0, 0x00]), None);
        assert_eq!(adts(&[0xFF, 0xF1, 0x3C, 0x80, 0, 0, 0]), None);
    }

    #[test]
    fn framing() {
        let mut f = Framer::default();
        // 中途接入：前面是半帧垃圾和 ID3
        let mut data = vec![0x12, 0xFF, 0x34];
        data.extend(b"ID3\x04\x00\x00\x00\x00\x00\x05abcde");
        for n in [300, 280, 310] {
            data.extend(adts_frame(n));
        }
        f.push(&data[..100]);
        assert!(f.next().is_none());
        f.push(&data[100..]);
        let lens: Vec<usize> = std::iter::from_fn(|| f.next().map(|(h, b)| {
            assert_eq!(h.len, b.len());
            b.len()
        }))
        .collect();
        // 最后一帧后面没有下一个帧头也照样输出（已同步）
        assert_eq!(lens, vec![300, 280, 310]);
        assert_eq!(f.locked().map(|h| h.rate), Some(44100));
        // 坏掉的一段之后重新同步
        let mut bad = vec![0u8; 50];
        bad.extend(adts_frame(200));
        bad.extend(adts_frame(200));
        f.push(&bad);
        assert_eq!(f.next().map(|(h, _)| h.len), Some(200));
    }
}
