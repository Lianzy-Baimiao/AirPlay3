//! MPEG-TS 解复用（HLS 电台的分段）：PAT → PMT → 第一路音频的 PES → 音频码流（ADTS / MP3 帧）。

/// PMT 里的音频流类型
fn audio_stream(t: u8) -> bool {
    // 0x03 / 0x04 = MPEG 音频，0x0F = AAC（ADTS），0x11 = AAC（LATM，不支持但先认下）
    matches!(t, 0x03 | 0x04 | 0x0F)
}

#[derive(Default)]
pub struct TsDemux {
    buf: Vec<u8>,
    pmt: Option<u16>,
    audio: Option<u16>,
    pes: Vec<u8>,
}

impl TsDemux {
    /// 送入 TS 数据，返回解出的音频码流
    pub fn push(&mut self, data: &[u8]) -> Vec<u8> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        let mut i = 0;
        while i + 188 <= self.buf.len() {
            if self.buf[i] != 0x47 {
                i += 1;
                continue;
            }
            let p: [u8; 188] = self.buf[i..i + 188].try_into().unwrap();
            self.packet(&p, &mut out);
            i += 188;
        }
        self.buf.drain(..i);
        out
    }

    fn packet(&mut self, p: &[u8; 188], out: &mut Vec<u8>) {
        let start = p[1] & 0x40 != 0;
        let pid = (((p[1] & 0x1F) as u16) << 8) | p[2] as u16;
        let afc = (p[3] >> 4) & 3;
        if afc & 1 == 0 {
            return; // 没有负载
        }
        let off = if afc == 3 { 5 + p[4] as usize } else { 4 };
        if off >= 188 {
            return;
        }
        let payload = &p[off..];
        if pid == 0 {
            self.pat(payload, start);
        } else if Some(pid) == self.pmt {
            self.read_pmt(payload, start);
        } else if Some(pid) == self.audio {
            if start {
                self.flush(out);
            }
            self.pes.extend_from_slice(payload);
        }
    }

    /// 段表（PAT / PMT）：跳过 pointer_field，返回段内容
    fn section(payload: &[u8], start: bool) -> Option<&[u8]> {
        let s = if start { payload.get(1 + *payload.first()? as usize..)? } else { return None };
        let len = (((s.get(1)? & 0x0F) as usize) << 8) | *s.get(2)? as usize;
        s.get(..3 + len)
    }

    fn pat(&mut self, payload: &[u8], start: bool) {
        let Some(s) = Self::section(payload, start) else { return };
        // 8 字节表头之后每 4 字节一个节目，最后 4 字节 CRC
        for e in s.get(8..s.len().saturating_sub(4)).unwrap_or(&[]).chunks_exact(4) {
            let prog = u16::from_be_bytes([e[0], e[1]]);
            if prog != 0 {
                self.pmt = Some((((e[2] & 0x1F) as u16) << 8) | e[3] as u16);
                return;
            }
        }
    }

    fn read_pmt(&mut self, payload: &[u8], start: bool) {
        let Some(s) = Self::section(payload, start) else { return };
        if s.len() < 12 {
            return;
        }
        let info = (((s[10] & 0x0F) as usize) << 8) | s[11] as usize;
        let mut i = 12 + info;
        let end = s.len().saturating_sub(4);
        while i + 5 <= end {
            let (t, pid) = (s[i], (((s[i + 1] & 0x1F) as u16) << 8) | s[i + 2] as u16);
            let es = (((s[i + 3] & 0x0F) as usize) << 8) | s[i + 4] as usize;
            if audio_stream(t) {
                if self.audio != Some(pid) {
                    self.pes.clear();
                }
                self.audio = Some(pid);
                return;
            }
            i += 5 + es;
        }
    }

    /// 一个完整的 PES：去掉 PES 头，输出码流
    fn flush(&mut self, out: &mut Vec<u8>) {
        let p = std::mem::take(&mut self.pes);
        if p.len() >= 9 && p[..3] == [0, 0, 1] {
            let start = 9 + p[8] as usize;
            if start <= p.len() {
                out.extend_from_slice(&p[start..]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(pid: u16, start: bool, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0x47, ((start as u8) << 6) | (pid >> 8) as u8, pid as u8, 0x10];
        v.extend_from_slice(payload);
        v.resize(188, 0xFF);
        v
    }

    #[test]
    fn demux() {
        // PAT：节目 1 → PMT PID 0x1000
        let pat = [0u8, 0x00, 0xB0, 0x0D, 0, 1, 0xC1, 0, 0, 0, 1, 0xF0, 0x00, 1, 2, 3, 4];
        // PMT：PCR PID，节目信息长度 0；流：0x1B 视频 PID 0x100，0x0F AAC PID 0x101
        let pmt = [0u8, 0x02, 0xB0, 0x17, 0, 1, 0xC1, 0, 0, 0xE1, 0x00, 0xF0, 0x00, 0x1B, 0xE1, 0x00, 0xF0, 0x00, 0x0F, 0xE1, 0x01, 0xF0, 0x00, 1, 2, 3, 4];
        // PES：起始码、流 ID、长度、标志、头长 5，后面是 5 字节 PTS，然后是码流
        let mut pes = vec![0, 0, 1, 0xC0, 0, 0, 0x80, 0x80, 5, 1, 2, 3, 4, 5];
        pes.extend(b"AUDIO-1");
        let mut ts = Vec::new();
        ts.extend(pkt(0, true, &pat));
        ts.extend(pkt(0x1000, true, &pmt));
        ts.extend(pkt(0x100, true, b"video"));
        ts.extend(pkt(0x101, true, &pes));
        let mut d = TsDemux::default();
        assert!(d.push(&ts[..300]).is_empty());
        // 下一个 PES 开始时上一个才输出
        let mut pes2 = vec![0, 0, 1, 0xC0, 0, 0, 0x80, 0x00, 0];
        pes2.extend(b"AUDIO-2");
        ts.extend(pkt(0x101, true, &pes2));
        let out = d.push(&ts[300..]);
        assert!(out.starts_with(b"AUDIO-1"), "{:?}", &out[..12.min(out.len())]);
        assert_eq!(d.audio, Some(0x101));
    }
}
