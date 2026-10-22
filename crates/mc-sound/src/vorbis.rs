//! Ogg Vorbis I decoder (the format of every 1.8.9 sound asset), written from the Xiph spec:
//! Ogg page demuxing, the three header packets, codebooks (with VQ lookup types 1 and 2),
//! floor type 1, residue types 0/1/2, channel coupling, an O(n log n) IMDCT and overlap-add.
//! Floor type 0 is not supported (no Minecraft asset uses it).

#[derive(Debug)]
pub struct Decoded {
    pub channels: usize,
    pub rate: u32,
    /// Interleaved samples in -1..1.
    pub samples: Vec<f32>,
}

// ---------------------------------------------------------------- Ogg

/// Splits the logical bitstream into packets (single stream assumed).
fn ogg_packets(data: &[u8]) -> Result<(Vec<Vec<u8>>, i64), String> {
    let mut packets = Vec::new();
    let mut granule = -1i64;
    let mut cur = Vec::new();
    let mut p = 0;
    while p + 27 <= data.len() {
        if &data[p..p + 4] != b"OggS" {
            return Err("bad Ogg capture pattern".into());
        }
        let nseg = data[p + 26] as usize;
        granule = i64::from_le_bytes(data[p + 6..p + 14].try_into().unwrap());
        let table = data.get(p + 27..p + 27 + nseg).ok_or("truncated page")?;
        let mut body = p + 27 + nseg;
        for &len in table {
            let seg = data.get(body..body + len as usize).ok_or("truncated segment")?;
            cur.extend_from_slice(seg);
            body += len as usize;
            if len < 255 {
                packets.push(std::mem::take(&mut cur));
            }
        }
        p = body;
    }
    if !cur.is_empty() {
        packets.push(cur);
    }
    Ok((packets, granule))
}

// ---------------------------------------------------------------- bit reader (LSB first)

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8]) -> Self {
        Self { d, pos: 0 }
    }
    fn read(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        let mut v: u64 = 0;
        for i in 0..n {
            let byte = *self.d.get(self.pos >> 3)?;
            let bit = (byte >> (self.pos & 7)) & 1;
            v |= (bit as u64) << i;
            self.pos += 1;
        }
        Some(v as u32)
    }
    fn bit(&mut self) -> Option<bool> {
        self.read(1).map(|b| b == 1)
    }
}

fn ilog(v: u32) -> u32 {
    32 - v.leading_zeros()
}

fn float32_unpack(x: u32) -> f32 {
    let mantissa = (x & 0x1fffff) as f64;
    let sign = x & 0x80000000;
    let exponent = ((x & 0x7fe00000) >> 21) as i32;
    let m = if sign != 0 { -mantissa } else { mantissa };
    (m * 2f64.powi(exponent - 788)) as f32
}

fn lookup1_values(entries: u32, dims: u32) -> u32 {
    let mut r = (entries as f64).powf(1.0 / dims as f64).floor() as u32;
    while (r + 1).pow(dims) <= entries {
        r += 1;
    }
    while r.pow(dims) > entries {
        r -= 1;
    }
    r
}

// ---------------------------------------------------------------- codebooks

struct Codebook {
    dims: usize,
    /// Huffman decode tree: nodes as [left, right]; >= 0 = node index, < 0 = -(entry+1).
    tree: Vec<[i32; 2]>,
    /// Flattened VQ vectors (entries × dims), empty for scalar books.
    vq: Vec<f32>,
}

impl Codebook {
    fn read(b: &mut Bits) -> Option<Codebook> {
        if b.read(24)? != 0x564342 {
            return None;
        }
        let dims = b.read(16)? as usize;
        let entries = b.read(24)? as usize;
        let mut lens = vec![0u8; entries];
        if !b.bit()? {
            let sparse = b.bit()?;
            for l in lens.iter_mut() {
                if !sparse || b.bit()? {
                    *l = b.read(5)? as u8 + 1;
                }
            }
        } else {
            let mut len = b.read(5)? + 1;
            let mut i = 0;
            while i < entries {
                let n = b.read(ilog((entries - i) as u32))? as usize;
                for l in lens.iter_mut().skip(i).take(n) {
                    *l = len as u8;
                }
                i += n;
                len += 1;
                if i > entries {
                    return None;
                }
            }
        }
        let lookup = b.read(4)?;
        let mut vq = Vec::new();
        if lookup == 1 || lookup == 2 {
            let min = float32_unpack(b.read(32)?);
            let delta = float32_unpack(b.read(32)?);
            let vbits = b.read(4)? + 1;
            let seq = b.bit()?;
            let n = if lookup == 1 { lookup1_values(entries as u32, dims as u32) as usize } else { entries * dims };
            let mut mult = Vec::with_capacity(n);
            for _ in 0..n {
                mult.push(b.read(vbits)? as f32);
            }
            vq = vec![0f32; entries * dims];
            for e in 0..entries {
                let mut last = 0f32;
                let mut idx_div = 1usize;
                for d in 0..dims {
                    let off = if lookup == 1 { (e / idx_div) % n } else { e * dims + d };
                    let v = mult[off] * delta + min + last;
                    vq[e * dims + d] = v;
                    if seq {
                        last = v;
                    }
                    if lookup == 1 {
                        idx_div *= n;
                    }
                }
            }
        } else if lookup != 0 {
            return None;
        }
        Some(Codebook { dims, tree: build_tree(&lens)?, vq })
    }

    fn decode_scalar(&self, b: &mut Bits) -> Option<usize> {
        let mut node = 0usize;
        if self.tree.is_empty() {
            return None;
        }
        loop {
            let next = self.tree[node][b.read(1)? as usize];
            if next < 0 {
                return Some((-next - 1) as usize);
            }
            if next == 0 {
                return None;
            }
            node = next as usize;
        }
    }

    fn decode_vq(&self, b: &mut Bits) -> Option<&[f32]> {
        let e = self.decode_scalar(b)?;
        self.vq.get(e * self.dims..(e + 1) * self.dims)
    }
}

/// Canonical-order Huffman assignment as the spec describes (lowest free codeword per length).
fn build_tree(lens: &[u8]) -> Option<Vec<[i32; 2]>> {
    let used: Vec<(usize, u8)> = lens.iter().enumerate().filter(|(_, &l)| l > 0).map(|(i, &l)| (i, l)).collect();
    let mut tree = vec![[0i32; 2]];
    if used.len() == 1 {
        // A single used entry: both branches decode it.
        let e = -(used[0].0 as i32) - 1;
        tree[0] = [e, e];
        return Some(tree);
    }
    // available[len] = next free codeword (MSB-first) at that length
    let mut available = [0u32; 33];
    let mut first = true;
    for &(entry, len) in &used {
        let len = len as usize;
        let code;
        if first {
            code = 0;
            for i in 1..=len {
                available[i] = 1u32 << (32 - i);
            }
            first = false;
        } else {
            let mut z = len;
            while z > 0 && available[z] == 0 {
                z -= 1;
            }
            if z == 0 {
                return None;
            }
            code = available[z];
            available[z] = 0;
            for i in z + 1..=len {
                available[i] = code + (1u32 << (32 - i));
            }
        }
        // walk/insert, MSB first
        let mut node = 0usize;
        for i in 0..len {
            let bit = ((code >> (31 - i)) & 1) as usize;
            if i == len - 1 {
                tree[node][bit] = -(entry as i32) - 1;
            } else {
                if tree[node][bit] <= 0 {
                    tree.push([0, 0]);
                    let n = (tree.len() - 1) as i32;
                    tree[node][bit] = n;
                }
                node = tree[node][bit] as usize;
            }
        }
    }
    Some(tree)
}

// ---------------------------------------------------------------- floor 1

struct Floor1 {
    partition_class: Vec<usize>,
    class_dims: Vec<usize>,
    class_subclasses: Vec<u32>,
    class_masterbook: Vec<usize>,
    subclass_books: Vec<Vec<i32>>,
    multiplier: u32,
    xs: Vec<u32>,
    /// Indices of xs sorted ascending.
    order: Vec<usize>,
    low: Vec<usize>,
    high: Vec<usize>,
}

impl Floor1 {
    fn read(b: &mut Bits) -> Option<Floor1> {
        let parts = b.read(5)? as usize;
        let mut partition_class = Vec::with_capacity(parts);
        let mut max_class: i32 = -1;
        for _ in 0..parts {
            let c = b.read(4)? as usize;
            max_class = max_class.max(c as i32);
            partition_class.push(c);
        }
        let nc = (max_class + 1) as usize;
        let (mut class_dims, mut class_subclasses, mut class_masterbook, mut subclass_books) = (vec![0; nc], vec![0; nc], vec![0; nc], vec![Vec::new(); nc]);
        for i in 0..nc {
            class_dims[i] = b.read(3)? as usize + 1;
            class_subclasses[i] = b.read(2)?;
            if class_subclasses[i] != 0 {
                class_masterbook[i] = b.read(8)? as usize;
            }
            for _ in 0..(1 << class_subclasses[i]) {
                subclass_books[i].push(b.read(8)? as i32 - 1);
            }
        }
        let multiplier = b.read(2)? + 1;
        let rangebits = b.read(4)?;
        let mut xs = vec![0, 1 << rangebits];
        for &c in &partition_class {
            for _ in 0..class_dims[c] {
                xs.push(b.read(rangebits)?);
            }
        }
        let mut order: Vec<usize> = (0..xs.len()).collect();
        order.sort_by_key(|&i| xs[i]);
        let (mut low, mut high) = (vec![0; xs.len()], vec![0; xs.len()]);
        for i in 2..xs.len() {
            let (mut lo, mut hi) = (0usize, 1usize);
            let (mut lx, mut hx) = (0u32, u32::MAX);
            for j in 0..i {
                if xs[j] > lx && xs[j] < xs[i] {
                    lx = xs[j];
                    lo = j;
                }
                if xs[j] < hx && xs[j] > xs[i] {
                    hx = xs[j];
                    hi = j;
                }
            }
            low[i] = lo;
            high[i] = hi;
        }
        Some(Floor1 { partition_class, class_dims, class_subclasses, class_masterbook, subclass_books, multiplier, xs, order, low, high })
    }

    /// Returns the Y list, or None when the floor is unused this packet.
    fn decode(&self, b: &mut Bits, books: &[Codebook]) -> Option<Option<Vec<i32>>> {
        if !b.bit()? {
            return Some(None);
        }
        let range = [256, 128, 86, 64][self.multiplier as usize - 1];
        let rb = ilog(range - 1);
        let mut y = vec![b.read(rb)? as i32, b.read(rb)? as i32];
        for &c in &self.partition_class {
            let cdim = self.class_dims[c];
            let cbits = self.class_subclasses[c];
            let csub = (1u32 << cbits) - 1;
            let mut cval = 0u32;
            if cbits > 0 {
                cval = books[self.class_masterbook[c]].decode_scalar(b)? as u32;
            }
            for _ in 0..cdim {
                let book = self.subclass_books[c][(cval & csub) as usize];
                cval >>= cbits;
                y.push(if book >= 0 { books[book as usize].decode_scalar(b)? as i32 } else { 0 });
            }
        }
        Some(Some(y))
    }

    fn curve(&self, y: &[i32], n: usize) -> Vec<f32> {
        let range = [256, 128, 86, 64][self.multiplier as usize - 1];
        let len = self.xs.len();
        let mut step2 = vec![false; len];
        let mut fy = vec![0i32; len];
        step2[0] = true;
        step2[1] = true;
        fy[0] = y[0];
        fy[1] = y[1];
        for i in 2..len {
            let (lo, hi) = (self.low[i], self.high[i]);
            let predicted = render_point(self.xs[lo] as i32, fy[lo], self.xs[hi] as i32, fy[hi], self.xs[i] as i32);
            let val = y[i];
            let highroom = range - predicted;
            let lowroom = predicted;
            let room = if highroom < lowroom { highroom } else { lowroom } * 2;
            if val != 0 {
                step2[lo] = true;
                step2[hi] = true;
                step2[i] = true;
                fy[i] = if val >= room {
                    if highroom > lowroom {
                        val - lowroom + predicted
                    } else {
                        predicted - val + highroom - 1
                    }
                } else if val % 2 == 1 {
                    predicted - (val + 1) / 2
                } else {
                    predicted + val / 2
                };
            } else {
                fy[i] = predicted;
            }
        }
        let mut out = vec![0f32; n];
        let m = self.multiplier as i32;
        let (mut lx, mut ly) = (0i32, fy[self.order[0]] * m);
        let mut hx = 0;
        for &i in &self.order[1..] {
            if step2[i] {
                hx = self.xs[i] as i32;
                let hy = fy[i] * m;
                render_line(lx, ly, hx, hy, &mut out);
                lx = hx;
                ly = hy;
            }
        }
        if (hx as usize) < n {
            for v in out.iter_mut().skip(hx as usize) {
                *v = FLOOR1_DB[ly.clamp(0, 255) as usize];
            }
        }
        out
    }
}

fn render_point(x0: i32, y0: i32, x1: i32, y1: i32, x: i32) -> i32 {
    let dy = y1 - y0;
    let adx = x1 - x0;
    let ady = dy.abs();
    let err = ady * (x - x0);
    let off = err / adx;
    if dy < 0 {
        y0 - off
    } else {
        y0 + off
    }
}

fn render_line(x0: i32, y0: i32, x1: i32, y1: i32, v: &mut [f32]) {
    let dy = y1 - y0;
    let adx = x1 - x0;
    let mut ady = dy.abs();
    let base = dy / adx;
    let sy = if dy < 0 { base - 1 } else { base + 1 };
    let mut x = x0;
    let mut y = y0;
    let mut err = 0;
    ady -= base.abs() * adx;
    if (x as usize) < v.len() {
        v[x as usize] = FLOOR1_DB[y.clamp(0, 255) as usize];
    }
    x += 1;
    while x < x1 {
        err += ady;
        if err >= adx {
            err -= adx;
            y += sy;
        } else {
            y += base;
        }
        if (x as usize) < v.len() {
            v[x as usize] = FLOOR1_DB[y.clamp(0, 255) as usize];
        }
        x += 1;
    }
}

// ---------------------------------------------------------------- residue

struct Residue {
    kind: u32,
    begin: usize,
    end: usize,
    partition_size: usize,
    classifications: usize,
    classbook: usize,
    books: Vec<[i32; 8]>,
}

impl Residue {
    fn read(b: &mut Bits, kind: u32) -> Option<Residue> {
        let begin = b.read(24)? as usize;
        let end = b.read(24)? as usize;
        let partition_size = b.read(24)? as usize + 1;
        let classifications = b.read(6)? as usize + 1;
        let classbook = b.read(8)? as usize;
        let mut cascade = Vec::with_capacity(classifications);
        for _ in 0..classifications {
            let low = b.read(3)?;
            let high = if b.bit()? { b.read(5)? } else { 0 };
            cascade.push(high * 8 + low);
        }
        let mut books = Vec::with_capacity(classifications);
        for c in cascade {
            let mut row = [-1i32; 8];
            for (j, r) in row.iter_mut().enumerate() {
                if c & (1 << j) != 0 {
                    *r = b.read(8)? as i32;
                }
            }
            books.push(row);
        }
        Some(Residue { kind, begin, end, partition_size, classifications, classbook, books })
    }

    /// Decodes into `out[ch]` (each length n/2) for channels with `do_decode[ch]`.
    fn decode(&self, b: &mut Bits, books: &[Codebook], half: usize, do_decode: &[bool], out: &mut [Vec<f32>]) -> Option<()> {
        let ch = out.len();
        if self.kind == 2 {
            if !do_decode.iter().any(|&d| d) {
                return Some(());
            }
            let mut flat = vec![vec![0f32; half * ch]];
            self.decode_inner(b, books, half * ch, &[true], &mut flat, 1)?;
            for (i, v) in flat[0].iter().enumerate() {
                out[i % ch][i / ch] += v;
            }
            return Some(());
        }
        self.decode_inner(b, books, half, do_decode, out, self.kind)
    }

    fn decode_inner(&self, b: &mut Bits, books: &[Codebook], n: usize, do_decode: &[bool], out: &mut [Vec<f32>], kind: u32) -> Option<()> {
        let ch = out.len();
        let begin = self.begin.min(n);
        let end = self.end.min(n);
        if end <= begin {
            return Some(());
        }
        let cb = &books[self.classbook];
        let per_word = cb.dims;
        let parts = (end - begin) / self.partition_size;
        let mut classes = vec![vec![0usize; parts + per_word]; ch];
        for pass in 0..8 {
            let mut p = 0;
            while p < parts {
                if pass == 0 {
                    for (j, cls) in classes.iter_mut().enumerate() {
                        if !do_decode[j] {
                            continue;
                        }
                        let mut temp = match cb.decode_scalar(b) {
                            Some(t) => t,
                            None => return Some(()),
                        };
                        for i in (0..per_word).rev() {
                            if p + i < cls.len() {
                                cls[p + i] = temp % self.classifications;
                            }
                            temp /= self.classifications;
                        }
                    }
                }
                for _ in 0..per_word {
                    if p >= parts {
                        break;
                    }
                    for j in 0..ch {
                        if !do_decode[j] {
                            continue;
                        }
                        let vqclass = classes[j][p];
                        let book = self.books[vqclass][pass];
                        if book >= 0 {
                            let bk = &books[book as usize];
                            let off = begin + p * self.partition_size;
                            let v = &mut out[j];
                            if kind == 0 {
                                let step = self.partition_size / bk.dims;
                                for i in 0..step {
                                    let Some(e) = bk.decode_vq(b) else { return Some(()) };
                                    for (k, val) in e.iter().enumerate() {
                                        if let Some(o) = v.get_mut(off + i + k * step) {
                                            *o += val;
                                        }
                                    }
                                }
                            } else {
                                let mut i = 0;
                                while i < self.partition_size {
                                    let Some(e) = bk.decode_vq(b) else { return Some(()) };
                                    for val in e {
                                        if let Some(o) = v.get_mut(off + i) {
                                            *o += val;
                                        }
                                        i += 1;
                                    }
                                }
                            }
                        }
                    }
                    p += 1;
                }
            }
        }
        Some(())
    }
}

// ---------------------------------------------------------------- setup

struct Mapping {
    coupling: Vec<(usize, usize)>,
    mux: Vec<usize>,
    submap_floor: Vec<usize>,
    submap_residue: Vec<usize>,
}

struct Mode {
    blockflag: bool,
    mapping: usize,
}

struct Setup {
    books: Vec<Codebook>,
    floors: Vec<Floor1>,
    residues: Vec<Residue>,
    mappings: Vec<Mapping>,
    modes: Vec<Mode>,
}

fn read_setup(p: &[u8], channels: usize) -> Option<Setup> {
    if p.first()? != &5 || &p[1..7] != b"vorbis" {
        return None;
    }
    let mut b = Bits::new(&p[7..]);
    let nbooks = b.read(8)? as usize + 1;
    let mut books = Vec::with_capacity(nbooks);
    for _ in 0..nbooks {
        books.push(Codebook::read(&mut b)?);
    }
    let ntime = b.read(6)? + 1;
    for _ in 0..ntime {
        b.read(16)?;
    }
    let nfloors = b.read(6)? + 1;
    let mut floors = Vec::new();
    for _ in 0..nfloors {
        if b.read(16)? != 1 {
            return None; // floor 0 unsupported
        }
        floors.push(Floor1::read(&mut b)?);
    }
    let nres = b.read(6)? + 1;
    let mut residues = Vec::new();
    for _ in 0..nres {
        let kind = b.read(16)?;
        residues.push(Residue::read(&mut b, kind)?);
    }
    let nmaps = b.read(6)? + 1;
    let mut mappings = Vec::new();
    for _ in 0..nmaps {
        b.read(16)?;
        let submaps = if b.bit()? { b.read(4)? as usize + 1 } else { 1 };
        let mut coupling = Vec::new();
        if b.bit()? {
            let steps = b.read(8)? + 1;
            let bits = ilog(channels as u32 - 1);
            for _ in 0..steps {
                coupling.push((b.read(bits)? as usize, b.read(bits)? as usize));
            }
        }
        b.read(2)?;
        let mut mux = vec![0; channels];
        if submaps > 1 {
            for m in mux.iter_mut() {
                *m = b.read(4)? as usize;
            }
        }
        let (mut submap_floor, mut submap_residue) = (Vec::new(), Vec::new());
        for _ in 0..submaps {
            b.read(8)?;
            submap_floor.push(b.read(8)? as usize);
            submap_residue.push(b.read(8)? as usize);
        }
        mappings.push(Mapping { coupling, mux, submap_floor, submap_residue });
    }
    let nmodes = b.read(6)? + 1;
    let mut modes = Vec::new();
    for _ in 0..nmodes {
        let blockflag = b.bit()?;
        b.read(16)?;
        b.read(16)?;
        modes.push(Mode { blockflag, mapping: b.read(8)? as usize });
    }
    Some(Setup { books, floors, residues, mappings, modes })
}

// ---------------------------------------------------------------- IMDCT

/// The spec IMDCT, y[i] = Σ X[k]·cos(2π/N·(i + ½ + N/4)(k + ½)), as one size-N complex FFT:
/// y[i] = Re(e^{jπ(i+n0)/N} · Σ_k X[k]·e^{j2π n0 k/N} · e^{j2π i k/N}) with n0 = ½ + N/4.
fn imdct(input: &[f32], n: usize) -> Vec<f32> {
    use std::f64::consts::PI;
    let n0 = 0.5 + n as f64 / 4.0;
    let mut re = vec![0f64; n];
    let mut im = vec![0f64; n];
    for (k, &x) in input.iter().take(n / 2).enumerate() {
        let t = 2.0 * PI * n0 * k as f64 / n as f64;
        // conjugated for the forward FFT (inverse via conj(FFT(conj)))
        re[k] = x as f64 * t.cos();
        im[k] = -(x as f64 * t.sin());
    }
    fft(&mut re, &mut im);
    let mut out = vec![0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let (sr, si) = (re[i], -im[i]);
        let t = PI * (i as f64 + n0) / n as f64;
        *o = (sr * t.cos() - si * t.sin()) as f32;
    }
    out
}

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    if n <= 1 {
        return;
    }
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (ws, wc) = ang.sin_cos();
        for i in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (ar, ai) = (re[i + k], im[i + k]);
                let (br, bi) = (re[i + k + len / 2], im[i + k + len / 2]);
                let (tr, ti) = (br * cr - bi * ci, br * ci + bi * cr);
                re[i + k] = ar + tr;
                im[i + k] = ai + ti;
                re[i + k + len / 2] = ar - tr;
                im[i + k + len / 2] = ai - ti;
                let ncr = cr * wc - ci * ws;
                ci = cr * ws + ci * wc;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// Reference O(n²) IMDCT per the Vorbis spec, used to validate the fast path in tests and as
/// the decoder's transform (block sizes are ≤ 2048 so this stays affordable for short sounds).
fn imdct_ref(input: &[f32], n: usize) -> Vec<f32> {
    let n2 = n / 2;
    let mut out = vec![0f32; n];
    // Precompute cos table lazily per call; fine for asset loading.
    for (i, o) in out.iter_mut().enumerate() {
        let mut s = 0f64;
        for (k, &x) in input.iter().take(n2).enumerate() {
            if x != 0.0 {
                s += x as f64 * (std::f64::consts::PI / n2 as f64 * (i as f64 + 0.5 + n2 as f64 / 2.0) * (k as f64 + 0.5)).cos();
            }
        }
        *o = s as f32;
    }
    out
}

fn window(n: usize, prev_long: bool, next_long: bool, long: bool, bs0: usize, bs1: usize) -> Vec<f32> {
    let mut w = vec![0f32; n];
    let vw = |x: f64, len: usize| -> f32 {
        let s = ((x + 0.5) / len as f64 * std::f64::consts::FRAC_PI_2).sin();
        (std::f64::consts::FRAC_PI_2 * s * s).sin() as f32
    };
    let (ls, le, rs, re) = if long {
        let lb = if prev_long { bs1 } else { bs0 };
        let rb = if next_long { bs1 } else { bs0 };
        (n / 4 - lb / 4, n / 4 + lb / 4, n * 3 / 4 - rb / 4, n * 3 / 4 + rb / 4)
    } else {
        (0, n / 2, n / 2, n)
    };
    for (i, v) in w.iter_mut().enumerate() {
        *v = if i < ls {
            0.0
        } else if i < le {
            vw((i - ls) as f64, le - ls)
        } else if i < rs {
            1.0
        } else if i < re {
            vw((re - i - 1) as f64, re - rs)
        } else {
            0.0
        };
    }
    w
}

// ---------------------------------------------------------------- decode

pub fn decode(data: &[u8]) -> Result<Decoded, String> {
    let (packets, last_granule) = ogg_packets(data)?;
    if packets.len() < 3 {
        return Err("missing Vorbis headers".into());
    }
    let id = &packets[0];
    if id.len() < 30 || id[0] != 1 || &id[1..7] != b"vorbis" {
        return Err("not a Vorbis stream".into());
    }
    let channels = id[11] as usize;
    let rate = u32::from_le_bytes([id[12], id[13], id[14], id[15]]);
    let bs0 = 1usize << (id[28] & 15);
    let bs1 = 1usize << (id[28] >> 4);
    let setup = read_setup(&packets[2], channels).ok_or("bad setup header")?;
    let mode_bits = ilog(setup.modes.len() as u32 - 1);

    let mut samples: Vec<f32> = Vec::new();
    let mut prev: Option<(Vec<Vec<f32>>, usize)> = None; // windowed previous block, its size
    let use_fast = std::env::var_os("RUSTMC_VORBIS_REF").is_none();
    for pkt in &packets[3..] {
        let mut b = Bits::new(pkt);
        if b.read(1) != Some(0) {
            continue;
        }
        let Some(mode_n) = b.read(mode_bits) else { continue };
        let Some(mode) = setup.modes.get(mode_n as usize) else { continue };
        let n = if mode.blockflag { bs1 } else { bs0 };
        let (mut prev_long, mut next_long) = (false, false);
        if mode.blockflag {
            prev_long = b.bit().unwrap_or(false);
            next_long = b.bit().unwrap_or(false);
        }
        let map = &setup.mappings[mode.mapping];
        let half = n / 2;
        // floors
        let mut floors: Vec<Option<Vec<f32>>> = Vec::with_capacity(channels);
        let mut ok = true;
        for c in 0..channels {
            let fl = &setup.floors[map.submap_floor[map.mux[c]]];
            match fl.decode(&mut b, &setup.books) {
                Some(Some(y)) => floors.push(Some(fl.curve(&y, half))),
                Some(None) => floors.push(None),
                None => {
                    ok = false;
                    floors.push(None);
                }
            }
        }
        if !ok {
            // A truncated packet: treat as silence.
            floors = vec![None; channels];
        }
        let mut no_residue: Vec<bool> = floors.iter().map(Option::is_none).collect();
        for &(m, a) in &map.coupling {
            if !no_residue[m] || !no_residue[a] {
                no_residue[m] = false;
                no_residue[a] = false;
            }
        }
        let mut res: Vec<Vec<f32>> = vec![vec![0f32; half]; channels];
        for (si, &ri) in map.submap_residue.iter().enumerate() {
            let chans: Vec<usize> = (0..channels).filter(|&c| map.mux[c] == si).collect();
            let mut sub: Vec<Vec<f32>> = chans.iter().map(|_| vec![0f32; half]).collect();
            let dd: Vec<bool> = chans.iter().map(|&c| !no_residue[c]).collect();
            let _ = setup.residues[ri].decode(&mut b, &setup.books, half, &dd, &mut sub);
            for (k, &c) in chans.iter().enumerate() {
                res[c] = std::mem::take(&mut sub[k]);
            }
        }
        // inverse coupling
        for &(m, a) in map.coupling.iter().rev() {
            for i in 0..half {
                let (mv, av) = (res[m][i], res[a][i]);
                let (nm, na) = if mv > 0.0 {
                    if av > 0.0 {
                        (mv, mv - av)
                    } else {
                        (mv + av, mv)
                    }
                } else if av > 0.0 {
                    (mv, mv + av)
                } else {
                    (mv - av, mv)
                };
                res[m][i] = nm;
                res[a][i] = na;
            }
        }
        // floor × residue, IMDCT, window
        let w = window(n, prev_long, next_long, mode.blockflag, bs0, bs1);
        let mut cur: Vec<Vec<f32>> = Vec::with_capacity(channels);
        for c in 0..channels {
            let spec: Vec<f32> = match &floors[c] {
                Some(f) => res[c].iter().zip(f).map(|(r, f)| r * f).collect(),
                None => vec![0f32; half],
            };
            let mut t = if use_fast { imdct(&spec, n) } else { imdct_ref(&spec, n) };
            for (v, wv) in t.iter_mut().zip(&w) {
                *v *= wv;
            }
            cur.push(t);
        }
        // Overlap-add: output runs from the previous window's center to this one's; prev
        // index pn/2 + j lines up with current index j + n/4 - pn/4.
        if let Some((p, pn)) = &prev {
            let pn = *pn;
            let out_len = pn / 4 + n / 4;
            let base = samples.len();
            samples.resize(base + out_len * channels, 0.0);
            for j in 0..out_len {
                for ch in 0..channels {
                    let pi = pn / 2 + j;
                    let pv = if pi < pn { p[ch][pi] } else { 0.0 };
                    let ci = j as isize + (n / 4) as isize - (pn / 4) as isize;
                    let cv = if ci >= 0 && (ci as usize) < n { cur[ch][ci as usize] } else { 0.0 };
                    samples[base + j * channels + ch] = pv + cv;
                }
            }
        }
        prev = Some((cur, n));
    }
    // The last page's granule position is the stream's total length in frames.
    if last_granule >= 0 && (last_granule as usize) * channels < samples.len() {
        samples.truncate(last_granule as usize * channels);
    }
    Ok(Decoded { channels, rate, samples })
}

/// Floor 1 inverse dB table (`floor1_inverse_dB_table`): a geometric series from
/// 1.0649863e-07 to 1.0.
static FLOOR1_DB: std::sync::LazyLock<[f32; 256]> = std::sync::LazyLock::new(|| {
    let mut t = [0f32; 256];
    let r = (1.0f64 / 1.0649863e-07).ln() / 255.0;
    for (i, v) in t.iter_mut().enumerate() {
        *v = (1.0649863e-07f64 * (r * i as f64).exp()) as f32;
    }
    t
});
