use mc_core::nbt::{Compound, Tag};
use mc_protocol::chunk::{self, WireSection};
use mc_protocol::frame;
use mc_protocol::packets::*;


mod seeds;
use seeds::*;

fn check_c(p: PlayClientbound) {
    let bytes = p.to_bytes();
    let back = PlayClientbound::read(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.name()));
    assert_eq!(back, p, "{}", p.name());
    assert_eq!(back.to_bytes(), bytes);
}

fn check_s(p: PlayServerbound) {
    let bytes = p.to_bytes();
    let back = PlayServerbound::read(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.name()));
    assert_eq!(back, p, "{}", p.name());
}

#[test]
fn every_serverbound_play_packet_round_trips() {
    let all = serverbound();
    let mut ids: Vec<i32> = all.iter().map(|p| p.id()).collect();
    ids.dedup();
    assert_eq!(ids, (0..=0x19).collect::<Vec<_>>());
    for p in all {
        check_s(p);
    }
}

#[test]
fn every_clientbound_play_packet_round_trips() {
    let all = clientbound();
    let mut ids: Vec<i32> = all.iter().map(|p| p.id()).collect();
    ids.dedup();
    assert_eq!(ids, (0..=0x49).collect::<Vec<_>>());
    for p in all {
        check_c(p);
    }
}

#[test]
fn login_and_status_round_trip() {
    let h = HandshakeServerbound::from(Handshake { protocol_version: 47, address: "localhost".into(), port: 25565, next_state: 2 });
    assert_eq!(HandshakeServerbound::read(&h.to_bytes()).unwrap(), h);
    for p in [
        LoginClientbound::from(LoginSuccess { uuid: uuid().to_string(), name: "Steve".into() }),
        EnableCompression { threshold: 256 }.into(),
        EncryptionRequest { server_id: String::new(), public_key: vec![1, 2, 3], verify_token: vec![4; 4] }.into(),
        LoginDisconnect { reason: "\"no\"".into() }.into(),
    ] {
        assert_eq!(LoginClientbound::read(&p.to_bytes()).unwrap(), p);
    }
    let s = StatusClientbound::from(StatusResponse { json: "{}".into() });
    assert_eq!(StatusClientbound::read(&s.to_bytes()).unwrap(), s);
    assert_eq!(StatusServerbound::read(&[0x00]).unwrap(), StatusServerbound::StatusRequest(StatusRequest {}));
}

#[test]
fn trailing_bytes_and_bad_ids_are_errors() {
    let mut b = PlayServerbound::from(KeepAliveC { key: 1 }).to_bytes();
    b.push(0);
    assert!(matches!(PlayServerbound::read(&b), Err(mc_protocol::Error::Trailing(1))));
    assert!(matches!(PlayServerbound::read(&[0x1a]), Err(mc_protocol::Error::BadPacketId(0x1a))));
    // Chat messages are limited to 100 characters.
    let long = PlayServerbound::from(ChatMessage { message: "a".repeat(101) }).to_bytes();
    assert!(PlayServerbound::read(&long).is_err());
}

#[test]
fn framing_with_and_without_compression() {
    let small = PlayClientbound::from(KeepAliveS { key: 7 }).to_bytes();
    let big = PlayClientbound::from(ChatS { json: "x".repeat(5000), position: 0 }).to_bytes();
    for threshold in [-1, 0, 256] {
        let mut stream = Vec::new();
        frame::encode(&small, threshold, &mut stream);
        frame::encode(&big, threshold, &mut stream);
        let mut d = frame::Decoder::new();
        d.threshold = threshold;
        // Feed one byte at a time to exercise partial frames.
        let mut got = Vec::new();
        for b in &stream {
            d.feed(std::slice::from_ref(b));
            while let Some(p) = d.next_packet().unwrap() {
                got.push(p);
            }
        }
        assert_eq!(got, vec![small.clone(), big.clone()], "threshold {threshold}");
        if threshold == 256 {
            assert!(stream.len() < big.len(), "large packet should be compressed");
            // Uncompressed packets below the threshold carry a 0 data length.
            assert_eq!(&stream[..3], &[3, 0, 0x00]);
        }
    }
}

#[test]
fn chunk_payload_round_trips() {
    let mut s = WireSection::empty(true);
    for i in 0..4096 {
        s.blocks[i] = ((i % 200) as u16) << 4 | (i % 16) as u16;
        chunk::set_nibble(&mut s.block_light, i, (i % 16) as u8);
        chunk::set_nibble(s.sky_light.as_mut().unwrap(), i, 15 - (i % 16) as u8);
    }
    let mut sections: [Option<WireSection>; 16] = Default::default();
    sections[3] = Some(s.clone());
    sections[15] = Some(s);
    let biomes = [21u8; 256];
    let (mask, data) = chunk::encode(&sections, Some(&biomes), true);
    assert_eq!(mask, 1 << 3 | 1 << 15);
    assert_eq!(data.len(), chunk::data_size(2, true, true));
    let back = chunk::decode(mask, &data, true, true).unwrap();
    assert_eq!(back.sections, sections);
    assert_eq!(back.biomes.as_deref(), Some(&biomes));
    // Little-endian block chars, low nibble first.
    assert_eq!(&data[2..4], &[(1u16 << 4 | 1).to_le_bytes()[0], 0]);
}

/// Vanilla reads and re-writes every sample packet (`oracle/PacketOracle.java`); the bytes must
/// match ours exactly. Skipped when Java 8 or the MCP classes are not available.
#[test]
fn vanilla_reencodes_every_packet_identically() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cp_file = root.join("../../tools/oracle/cp.txt");
    let java = std::path::PathBuf::from(r"C:\Program Files\Eclipse Adoptium\jdk-8.0.472.8-hotspot\bin");
    let Ok(cp) = std::fs::read_to_string(&cp_file) else {
        eprintln!("skipping: no {}", cp_file.display());
        return;
    };
    if !java.join("java.exe").exists() {
        eprintln!("skipping: no Java 8");
        return;
    }
    let out_dir = root.join("../../target/protocol-oracle");
    std::fs::create_dir_all(&out_dir).unwrap();
    let status = std::process::Command::new(java.join("javac.exe"))
        .args(["-nowarn", "-cp", cp.trim(), "-d"])
        .arg(&out_dir)
        .arg(root.join("oracle/PacketOracle.java"))
        .status()
        .unwrap();
    assert!(status.success(), "javac failed");

    let mut input = Vec::new();
    let mut expected = Vec::new();
    for p in clientbound() {
        expected.push((0u8, p.name(), p.to_bytes()));
    }
    for p in serverbound() {
        expected.push((1u8, p.name(), p.to_bytes()));
    }
    for (dir, _, b) in &expected {
        input.push(*dir);
        input.extend_from_slice(&(b.len() as u32).to_be_bytes());
        input.extend_from_slice(b);
    }
    let (inp, outp) = (out_dir.join("in.bin"), out_dir.join("out.bin"));
    std::fs::write(&inp, &input).unwrap();
    let status = std::process::Command::new(java.join("java.exe"))
        .arg("-cp")
        .arg(format!("{};{}", out_dir.display(), cp.trim()))
        .arg("PacketOracle")
        .arg(&inp)
        .arg(&outp)
        .status()
        .unwrap();
    assert!(status.success(), "oracle failed");
    let got = std::fs::read(&outp).unwrap();
    let mut p = 0;
    let mut bad = Vec::new();
    for (dir, name, want) in &expected {
        let d = got[p];
        p += 1;
        if d == 255 {
            let n = u16::from_be_bytes([got[p], got[p + 1]]) as usize;
            bad.push(format!("{name}: vanilla error {}", String::from_utf8_lossy(&got[p + 2..p + 2 + n])));
            p += 2 + n;
            continue;
        }
        let n = u32::from_be_bytes(got[p..p + 4].try_into().unwrap()) as usize;
        let b = &got[p + 4..p + 4 + n];
        p += 4 + n;
        assert_eq!(d, *dir);
        if b != &want[..] {
            let at = b.iter().zip(want.iter()).position(|(x, y)| x != y).unwrap_or(b.len().min(want.len()));
            bad.push(format!("{name}: differs at byte {at} (vanilla {} bytes, ours {})", b.len(), want.len()));
        }
    }
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
    eprintln!("{} packets identical to vanilla", expected.len());
}

/// Deterministic xorshift so failures reproduce.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    /// Bytes biased towards values that stress lengths and varints.
    fn byte(&mut self) -> u8 {
        match self.below(6) {
            0 => 0,
            1 => 0xff,
            2 => 0x7f,
            3 => 0x80,
            _ => self.next() as u8,
        }
    }
}

/// Random and mutated packet streams must produce errors, never panics.
#[test]
fn fuzz_decoders_never_panic() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let seeds: Vec<Vec<u8>> = clientbound().iter().map(|p| p.to_bytes()).collect();
    let sb: Vec<Vec<u8>> = serverbound().iter().map(|p| p.to_bytes()).collect();
    let mut ok = 0u32;
    for i in 0..300_000u32 {
        let mut b: Vec<u8> = match i % 3 {
            0 => {
                let n = rng.below(64);
                let mut v = vec![rng.below(0x49) as u8];
                v.extend((0..n).map(|_| rng.byte()));
                v
            }
            1 => seeds[rng.below(seeds.len())].clone(),
            _ => sb[rng.below(sb.len())].clone(),
        };
        if i % 3 != 0 {
            for _ in 0..1 + rng.below(4) {
                match rng.below(4) {
                    0 if !b.is_empty() => {
                        let k = rng.below(b.len());
                        b[k] = rng.byte();
                    }
                    1 if !b.is_empty() => {
                        let k = rng.below(b.len());
                        b.truncate(k);
                    }
                    2 => {
                        let k = rng.below(b.len() + 1);
                        b.insert(k, rng.byte());
                    }
                    _ if !b.is_empty() => {
                        let k = rng.below(b.len());
                        b.remove(k);
                    }
                    _ => {}
                }
            }
        }
        if PlayClientbound::read(&b).is_ok() {
            ok += 1;
        }
        let _ = PlayServerbound::read(&b);
        let _ = LoginClientbound::read(&b);
        let _ = StatusClientbound::read(&b);
    }
    assert!(ok > 0);
}

/// Hostile frame streams: wild lengths, bogus compression headers, garbage zlib.
#[test]
fn fuzz_frame_decoder_never_panics() {
    let mut rng = Rng(0xD1B54A32D192ED03);
    for i in 0..100_000u32 {
        let mut d = frame::Decoder::new();
        d.threshold = if i % 2 == 0 { -1 } else { rng.below(300) as i32 - 1 };
        let n = rng.below(48);
        let data: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
        let mut from = 0;
        while from < data.len() {
            let to = (from + 1 + rng.below(8)).min(data.len());
            d.feed(&data[from..to]);
            from = to;
            while let Ok(Some(_)) = d.next_packet() {}
        }
    }
    // Valid zlib header, truncated / lying about size.
    let mut d = frame::Decoder::new();
    d.threshold = 10;
    d.feed(&[6, 0xff, 0xff, 0x7f, 0x78, 0x9c, 0x00]);
    assert!(d.next_packet().is_err());
}

#[test]
fn varints_past_five_bytes_error() {
    assert!(mc_protocol::Reader::new(&[0x80; 8]).varint().is_err());
    assert!(mc_protocol::Reader::new(&[0x80; 12]).varlong().is_err());
}
