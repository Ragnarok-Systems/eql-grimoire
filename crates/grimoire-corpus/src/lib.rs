//! The corpus: items, recipes, components, sources — shipped, not served.
//!
//! # Why this shape
//!
//! Cloudflare's free tier gives a Worker 10 ms of CPU and 100k requests a day. One database
//! query per item lookup, at guild scale, spends that on nothing. So the corpus is a **static
//! object on a CDN**: content-addressed, so a rebuild is a new key and nothing ever needs
//! invalidating, and **range-readable**, so a phone looking up one bracer fetches a few
//! kilobytes instead of the whole file.
//!
//! # Relationship to Akashic RFC 42
//!
//! RFC 42 proposes exactly this as a fourth Akashic mode, with provenance chained back to the
//! audited source. **It is a proposal — `akashic publish` does not exist yet**, so this is a
//! small standalone implementation of the same shape, and [`Reader`] is a trait with one
//! method so the artifact can be swapped for real publish output without touching a caller.
//! The manifest already carries a `provenance` field for the chain root to land in.
//!
//! # Layout
//!
//! ```text
//!   [ records… ][ index block 0 ][ index block 1 ] … [ directory ][ footer: 16 bytes ]
//! ```
//!
//! Records are JSON, sorted by key. The index is **two-level**, and that is not premature:
//! a flat index over a real corpus is megabytes, so fetching it to look up one bracer costs
//! more than the record does. Instead the index is cut into blocks of
//! [`INDEX_BLOCK`] keys, and a small directory holds the first key of each block. A lookup is
//!
//! ```text
//!   footer (16 bytes) → directory (kilobytes) → one index block → one record
//! ```
//!
//! four range requests, none of which grows with the size of the corpus. The footer is
//! fixed-size and last because a reader that has never seen the file can ask for the final
//! sixteen bytes without knowing anything about it.
//!
//! Keys and offsets are packed binary rather than JSON: at index scale the punctuation costs
//! more than the data.

use std::collections::BTreeMap;

pub mod hash;

/// `(key, offset, length)` — one row of either index level.
pub type Entry = (String, u64, u32);

/// Bumped whenever the byte layout changes. A reader refuses anything it does not know.
pub const FORMAT_VERSION: u16 = 2;
pub const MAGIC: [u8; 6] = *b"GRIMOR";
pub const FOOTER_LEN: usize = 16;
/// Keys per index block. Sets the directory/block size trade: bigger blocks mean a smaller
/// directory and a larger single fetch.
pub const INDEX_BLOCK: usize = 256;

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub format: u16,
    /// SHA-256 of everything before the footer, hex. See [`hash`] for why not BLAKE3 yet.
    pub content_hash: String,
    pub records: u32,
    /// Where the bytes came from, and when. Free text now; the RFC 42 chain root later.
    pub provenance: Vec<String>,
    pub built: String,
}

/// Anything that can hand back a byte range. A local file, an HTTP `Range` request, an R2
/// object — the reader does not care, which is the whole point of the artifact.
pub trait Fetch {
    fn range(&self, offset: u64, len: usize) -> Result<Vec<u8>, CorpusError>;
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CorpusError {
    /// Not a Grimoire artifact.
    BadMagic,
    /// Built by a newer writer than this reader understands.
    UnknownFormat(u16),
    Truncated {
        want: usize,
        got: usize,
    },
    Corrupt(&'static str),
    NotFound,
}

impl std::fmt::Display for CorpusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CorpusError::BadMagic => write!(f, "not a Grimoire corpus"),
            CorpusError::UnknownFormat(v) => {
                write!(f, "corpus format {v} is newer than this reader")
            }
            CorpusError::Truncated { want, got } => write!(f, "wanted {want} bytes, got {got}"),
            CorpusError::Corrupt(what) => write!(f, "corpus is corrupt: {what}"),
            CorpusError::NotFound => write!(f, "no such key"),
        }
    }
}
impl std::error::Error for CorpusError {}

/// Builds an artifact.
#[derive(Default, Debug)]
pub struct Writer {
    records: BTreeMap<String, Vec<u8>>,
    provenance: Vec<String>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }

    /// Note where some of this data came from. Ends up in the manifest.
    pub fn source(&mut self, note: impl Into<String>) -> &mut Self {
        self.provenance.push(note.into());
        self
    }

    pub fn put<T: serde::Serialize>(&mut self, key: impl Into<String>, value: &T) -> &mut Self {
        let bytes = serde_json::to_vec(value).expect("record is not serialisable");
        self.records.insert(key.into(), bytes);
        self
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Serialise. Returns the bytes and the manifest describing them.
    ///
    /// Deterministic: the same records produce the same bytes, whatever order they were put
    /// in. Otherwise two builds of an identical corpus would content-address differently and
    /// the address would stop meaning "these bytes".
    pub fn finish(&self, built: &str) -> (Vec<u8>, Manifest) {
        let mut body = Vec::new();
        let mut entries: Vec<(&str, u64, u32)> = Vec::with_capacity(self.records.len());
        for (k, v) in &self.records {
            entries.push((k.as_str(), body.len() as u64, v.len() as u32));
            body.extend_from_slice(v);
        }

        // Index blocks, then a directory naming the first key of each.
        let mut directory: Vec<(&str, u64, u32)> = Vec::new();
        for chunk in entries.chunks(INDEX_BLOCK) {
            let at = body.len() as u64;
            let mut block = Vec::new();
            for (k, off, len) in chunk {
                put_entry(&mut block, k, *off, *len);
            }
            body.extend_from_slice(&block);
            directory.push((chunk[0].0, at, block.len() as u32));
        }

        let dir_at = body.len() as u64;
        for (k, off, len) in &directory {
            put_entry(&mut body, k, *off, *len);
        }

        let content_hash = hash::hex(&body);

        // footer: MAGIC(6) version(2) directory_offset(8)
        body.extend_from_slice(&MAGIC);
        body.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        body.extend_from_slice(&dir_at.to_le_bytes());

        let manifest = Manifest {
            format: FORMAT_VERSION,
            content_hash,
            records: self.records.len() as u32,
            provenance: self.provenance.clone(),
            built: built.to_string(),
        };
        (body, manifest)
    }
}

/// `u16 key_len | key | u64 offset | u32 len`
fn put_entry(out: &mut Vec<u8>, key: &str, offset: u64, len: u32) {
    let k = key.as_bytes();
    assert!(k.len() <= u16::MAX as usize, "key is absurdly long");
    out.extend_from_slice(&(k.len() as u16).to_le_bytes());
    out.extend_from_slice(k);
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
}

/// Decode a run of entries written by [`put_entry`].
///
/// Every length is checked against what is actually left, so a corrupt or hostile artifact
/// produces an error rather than a panic or a wild read.
fn read_entries(mut buf: &[u8]) -> Result<Vec<Entry>, CorpusError> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        if buf.len() < 2 {
            return Err(CorpusError::Corrupt("index entry is cut short"));
        }
        let klen = u16::from_le_bytes([buf[0], buf[1]]) as usize;
        let need = 2 + klen + 12;
        if buf.len() < need {
            return Err(CorpusError::Corrupt("index entry runs past the block"));
        }
        let key = std::str::from_utf8(&buf[2..2 + klen])
            .map_err(|_| CorpusError::Corrupt("index key is not utf-8"))?
            .to_string();
        let off = u64::from_le_bytes(buf[2 + klen..10 + klen].try_into().unwrap());
        let len = u32::from_le_bytes(buf[10 + klen..14 + klen].try_into().unwrap());
        out.push((key, off, len));
        buf = &buf[need..];
    }
    Ok(out)
}

/// Reads an artifact through however few range requests it can manage.
///
/// Opening costs two reads and holds only the directory, so memory does not grow with the
/// corpus. Index blocks are cached as they are touched, which is what makes a prefix walk or
/// a burst of nearby lookups cheap without ever loading the whole index.
#[derive(Debug)]
pub struct Reader<F: Fetch> {
    fetch: F,
    /// first key of block → (offset, len)
    directory: Vec<Entry>,
    /// Where the index starts; nothing above this is a record.
    records_end: u64,
    blocks: std::cell::RefCell<BTreeMap<usize, Vec<Entry>>>,
}

impl<F: Fetch> Reader<F> {
    /// Two range reads: the footer, then the directory.
    pub fn open(fetch: F) -> Result<Reader<F>, CorpusError> {
        let total = fetch.len();
        if total < FOOTER_LEN as u64 {
            return Err(CorpusError::Truncated {
                want: FOOTER_LEN,
                got: total as usize,
            });
        }
        let footer = fetch.range(total - FOOTER_LEN as u64, FOOTER_LEN)?;
        if footer.len() != FOOTER_LEN {
            return Err(CorpusError::Truncated {
                want: FOOTER_LEN,
                got: footer.len(),
            });
        }
        if footer[..6] != MAGIC {
            return Err(CorpusError::BadMagic);
        }
        let version = u16::from_le_bytes([footer[6], footer[7]]);
        if version != FORMAT_VERSION {
            return Err(CorpusError::UnknownFormat(version));
        }
        let dir_at = u64::from_le_bytes(footer[8..16].try_into().unwrap());
        let dir_end = total - FOOTER_LEN as u64;
        if dir_at > dir_end {
            return Err(CorpusError::Corrupt("directory offset past end of file"));
        }

        let raw = fetch.range(dir_at, (dir_end - dir_at) as usize)?;
        let directory = read_entries(&raw)?;
        for (_, off, len) in &directory {
            if off.saturating_add(*len as u64) > dir_at {
                return Err(CorpusError::Corrupt("index block runs into the directory"));
            }
        }
        // The first index block begins where the records end.
        let records_end = directory.first().map(|(_, o, _)| *o).unwrap_or(dir_at);

        Ok(Reader {
            fetch,
            directory,
            records_end,
            blocks: Default::default(),
        })
    }

    /// The underlying fetcher. Mostly so tests can assert how much was read.
    pub fn fetch_ref(&self) -> &F {
        &self.fetch
    }

    /// Index blocks, not records. A corpus of 50,000 items has around 200.
    pub fn index_blocks(&self) -> usize {
        self.directory.len()
    }

    pub fn is_empty(&self) -> bool {
        self.directory.is_empty()
    }

    /// Which block a key would live in, if it lives anywhere.
    fn block_for(&self, key: &str) -> Option<usize> {
        match self
            .directory
            .binary_search_by(|(k, _, _)| k.as_str().cmp(key))
        {
            Ok(i) => Some(i),
            // Before the first key in the corpus.
            Err(0) => None,
            Err(i) => Some(i - 1),
        }
    }

    fn block(&self, i: usize) -> Result<Vec<Entry>, CorpusError> {
        if let Some(b) = self.blocks.borrow().get(&i) {
            return Ok(b.clone());
        }
        let (_, off, len) = &self.directory[i];
        let raw = self.fetch.range(*off, *len as usize)?;
        let entries = read_entries(&raw)?;
        for (_, o, l) in &entries {
            if o.saturating_add(*l as u64) > self.records_end {
                return Err(CorpusError::Corrupt("record extends past the index"));
            }
        }
        self.blocks.borrow_mut().insert(i, entries.clone());
        Ok(entries)
    }

    fn locate(&self, key: &str) -> Result<Option<(u64, u32)>, CorpusError> {
        let Some(i) = self.block_for(key) else {
            return Ok(None);
        };
        Ok(self
            .block(i)?
            .into_iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, o, l)| (o, l)))
    }

    pub fn contains(&self, key: &str) -> bool {
        matches!(self.locate(key), Ok(Some(_)))
    }

    /// Every key, in order. Walks every index block, so it is the expensive call.
    pub fn keys(&self) -> Result<Vec<String>, CorpusError> {
        let mut out = Vec::new();
        for i in 0..self.directory.len() {
            out.extend(self.block(i)?.into_iter().map(|(k, _, _)| k));
        }
        Ok(out)
    }

    /// Number of records. Requires walking the index blocks.
    pub fn len(&self) -> Result<usize, CorpusError> {
        let mut n = 0;
        for i in 0..self.directory.len() {
            n += self.block(i)?.len();
        }
        Ok(n)
    }

    /// One block read (usually cached) plus one record read.
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<T, CorpusError> {
        let (off, len) = self.locate(key)?.ok_or(CorpusError::NotFound)?;
        let raw = self.fetch.range(off, len as usize)?;
        serde_json::from_slice(&raw).map_err(|_| CorpusError::Corrupt("record is not readable"))
    }

    /// Keys under a prefix, e.g. `recipe/`.
    ///
    /// Starts at the block the prefix would begin in and stops at the first key that does not
    /// match, so asking for recipes never reads the item blocks.
    pub fn prefix(&self, prefix: &str) -> Result<Vec<String>, CorpusError> {
        let start = self.block_for(prefix).unwrap_or(0);
        let mut out = Vec::new();
        for i in start..self.directory.len() {
            // Once a block starts past the prefix, nothing later can match.
            if self.directory[i].0.as_str() > prefix && !self.directory[i].0.starts_with(prefix) {
                break;
            }
            for (k, _, _) in self.block(i)? {
                if k.starts_with(prefix) {
                    out.push(k);
                } else if k.as_str() > prefix && !out.is_empty() {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }
}

/// A whole artifact already in memory. What the corpus builder and the tests use.
#[derive(Clone, Debug)]
pub struct InMemory(pub Vec<u8>);

impl Fetch for InMemory {
    fn range(&self, offset: u64, len: usize) -> Result<Vec<u8>, CorpusError> {
        let start = offset as usize;
        let end = start.saturating_add(len);
        if end > self.0.len() {
            return Err(CorpusError::Truncated {
                want: end,
                got: self.0.len(),
            });
        }
        Ok(self.0[start..end].to_vec())
    }
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
}

/// Counts range requests, so a test can assert that a lookup did not turn into a scan.
#[derive(Debug)]
pub struct Counting<F: Fetch> {
    pub inner: F,
    pub reads: std::cell::Cell<u32>,
    pub bytes: std::cell::Cell<u64>,
}

impl<F: Fetch> Counting<F> {
    pub fn new(inner: F) -> Self {
        Counting {
            inner,
            reads: Default::default(),
            bytes: Default::default(),
        }
    }
}

impl<F: Fetch> Fetch for Counting<F> {
    fn range(&self, offset: u64, len: usize) -> Result<Vec<u8>, CorpusError> {
        self.reads.set(self.reads.get() + 1);
        self.bytes.set(self.bytes.get() + len as u64);
        self.inner.range(offset, len)
    }
    fn len(&self) -> u64 {
        self.inner.len()
    }
}
