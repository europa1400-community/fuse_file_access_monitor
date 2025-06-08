
use chrono::Utc;
use fuser::{
    consts, FileAttr, FileType, Filesystem, KernelConfig, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, Request
};
use libc::ENOENT;
use std::ffi::{c_int, OsStr};
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

const TTL: Duration = Duration::from_secs(1); // 1 second

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct ReadEvent {
    pub file: std::sync::Arc<String>,
    pub offset: usize,
    pub size: usize
}

impl std::fmt::Display for ReadEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Reading {} bytes (offset {}) from {}", self.size, self.offset, self.file)
    }
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub enum EventType {
    Read(ReadEvent)
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(event) => write!(f, "{}", event)
        }
    }
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct Event {
    pub time: chrono::DateTime<Utc>,
    pub event : EventType
}

impl std::fmt::Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.time, self.event)
    }
}

#[derive(Eq, PartialEq, Debug)]
struct Directory {
    root : Entry,
    inode_ctr: u64
}

impl Directory {
    pub fn new(dir: &str) -> Self {
        let mut inode_ctr = 1;
        Self {
            root: Entry::new(dir, &mut inode_ctr),
            inode_ctr
        }
    }
    pub fn create_file(&mut self, parent : u64, name : &str) -> Result<&Entry,()> {
        match self.root.find_ino_mut(parent) {
            Some(parent) => {
                match &mut parent.info {
                    EntryInfo::File(_) => {
                        Err(())
                    }
                    EntryInfo::Directory(entries) => {
                        entries.push(Entry {
                            name: Arc::new(name.into()),
                            full_path: Data::Memory(Vec::new()),
                            inode: self.inode_ctr,
                            info: EntryInfo::File(0)
                        });
                        self.inode_ctr += 1;
                        Ok(entries.last().unwrap())
                    }
                }
            }
            None => {
                Err(())
            }
        }
    }
}

impl Entry {
    pub fn new(dir: &str, inode_ctr : &mut u64) -> Self {
        *inode_ctr += 1;
        Self {
            full_path: Data::FilePath(dir.to_string()),
            name: Arc::new(String::new()),
            info: EntryInfo::Directory(Self::build_directory(dir, inode_ctr)),
            inode: 1
        }
    }

    fn find_ino(&self, ino: u64) -> Option<&Entry> {
        if ino==1 {
            Some(self)
        } else {
            match &self.info {
                EntryInfo::Directory(contents) => {
                    Self::find_ino_internal(contents, ino)
                }
                EntryInfo::File(_) => None
            }
        }
    }

    fn find_ino_mut(&mut self, ino: u64) -> Option<&mut Entry> {
        if ino==self.inode {
            Some(self)
        } else {
            match &mut self.info {
                EntryInfo::Directory(contents) => {
                    Self::find_ino_mut_internal(contents, ino)
                }
                EntryInfo::File(_) => None
            }
        }
    }

    fn find_name(&self, name : &str) -> Option<&Self> {
        match &self.info {
            EntryInfo::Directory(entries) => {
                entries.iter().filter(|e| e.name.to_lowercase()==name.to_lowercase()).next()
            }
            EntryInfo::File(_) => None
        }
    }

    fn find_ino_internal(directory : &Vec<Entry>, ino : u64) -> Option<&Entry> {
        match directory.iter().filter(|e| e.inode==ino).next() {
            Some(result) => Some(result),
            None => {
                directory.iter().filter_map(|e| {
                    match &e.info {
                        EntryInfo::File(_) => None,
                        EntryInfo::Directory(entries) => Self::find_ino_internal(&entries, ino)
                    }
                }).next()
            }
        }
    }

    fn find_ino_mut_internal(directory : &mut Vec<Entry>, ino : u64) -> Option<&mut Entry> {
        for entry in directory.iter_mut().filter(|e| e.inode==ino || e.info.is_dir()) {
            if entry.inode == ino {
                return Some(entry);
            } else {
                match &mut entry.info {
                    EntryInfo::Directory(contents) => {
                        if let Some(entry) = Self::find_ino_mut_internal(contents, ino) {
                            return Some(entry);
                        }
                    }
                    EntryInfo::File(_) => {}
                }
            }
        }
        None
    }
    
    fn build_directory(dir: &str, inode_offset: &mut u64) -> Vec<Entry> {
        let path = std::path::PathBuf::from(dir);
        let mut entries = Vec::new();

        // Read the directory
        for entry in std::fs::read_dir(path).expect("Failed to read directory") {
            let entry = entry.expect("Failed to read directory entry");
            let file_name = entry.file_name();
            let name = file_name.to_str().unwrap_or("unknown").to_string();

            // Skip . and ..
            if name == "." || name == ".." {
                continue;
            }

            let path = entry.path();
            let abs_path = path.canonicalize().expect("Failed to get canonical path");
            let full_path = abs_path.to_str().unwrap_or("unknown").to_string();

            let meta = entry.metadata().expect("Failed to get metadata");

            if meta.is_dir() {
                // Recursively build the subdirectory
                let sub_entries = Self::build_directory(full_path.as_str(), inode_offset);
                entries.push(Entry {
                    name: Arc::new(name),
                    full_path: Data::FilePath(full_path),
                    inode: *inode_offset,
                    info: EntryInfo::Directory(sub_entries),
                });
                *inode_offset += 1;
            } else {
                // It's a file
                let size = meta.len();
                entries.push(Entry {
                    name: Arc::new(name),
                    full_path: Data::FilePath(full_path),
                    inode: *inode_offset,
                    info: EntryInfo::File(size),
                });
                *inode_offset += 1;
            }
        }

        entries
    }

}

#[derive(Eq, PartialEq, Debug)]
enum Data {
    FilePath(String),
    Memory(Vec<u8>)
}

impl Data {
    fn read(&self, buffer : &mut [u8], offset : usize) -> std::io::Result<usize> {
        match self {
            Data::FilePath(path) => {
                match File::open(path) {
                    Ok(file) => {
                        file.read_at(buffer, offset as u64)
                    }
                    Err(err) => {
                        Err(err)
                    }
                }
            }
            Data::Memory(data) => {
                let src_slice = &data[offset..offset+buffer.len()];
                buffer.copy_from_slice(src_slice);
                Ok(src_slice.len())
            }
        }
    }
}

#[derive(Eq, PartialEq, Debug)]
struct Entry {
    pub name: std::sync::Arc<String>,
    pub full_path: Data,
    pub inode : u64,
    pub info: EntryInfo
}

#[derive(Eq, PartialEq, Debug)]
enum EntryInfo {
    Directory(Vec<Entry>),
    File(u64) // file holds the size in bytes
}

impl Entry {
    pub fn get_fileattr(&self) -> FileAttr {
        match &self.info {
            EntryInfo::File(size) => {
                FileAttr {
                    ino: self.inode,
                    size: *size,
                    blocks: 1,
                    atime: UNIX_EPOCH, // 1970-01-01 00:00:00
                    mtime: UNIX_EPOCH,
                    ctime: UNIX_EPOCH,
                    crtime: UNIX_EPOCH,
                    kind: FileType::RegularFile,
                    perm: 0o755,
                    nlink: 1,
                    uid: 333,
                    gid: 333,
                    rdev: 0,
                    flags: 0,
                    blksize: 512,
                }
            }
            EntryInfo::Directory(_entries) => {
                FileAttr {
                    ino: self.inode,
                    size: 0,
                    blocks: 0,
                    atime: UNIX_EPOCH, // 1970-01-01 00:00:00
                    mtime: UNIX_EPOCH,
                    ctime: UNIX_EPOCH,
                    crtime: UNIX_EPOCH,
                    kind: FileType::Directory,
                    perm: 0o755,
                    nlink: 2,
                    uid: 1000,
                    gid: 1000,
                    rdev: 0,
                    flags: 0,
                    blksize: 512,
                }
            }
        }
    }
}

impl EntryInfo {
    pub fn is_dir(&self) -> bool {
        match self {
            Self::Directory(_) => true,
            Self::File(_) => false
        }
    }
    pub fn is_file(&self) -> bool {
        match self {
            Self::Directory(_) => false,
            Self::File(_) => true
        }
    }
}




#[derive(Debug)]
pub struct FileAccessTrackingFs {
    directory: Directory,
    event_sender : tokio::sync::mpsc::Sender<Event>,
    _uid: u32,
    _gid: u32
}

impl FileAccessTrackingFs {
    pub fn new(source : &str, event_sender : tokio::sync::mpsc::Sender<Event>) -> Self {
        let uid = unsafe { libc::getuid() };
        let gid = unsafe { libc::getgid() };
        
        let directory = Directory::new(source);

        Self {
            directory,
            event_sender,
            _uid : uid,
            _gid : gid
        }
    }
}

impl Filesystem for FileAccessTrackingFs {
    fn init(
        &mut self,
        _req: &Request,
        config: &mut KernelConfig,
    ) -> std::result::Result<(), c_int> {
        config.add_capabilities(consts::FUSE_PASSTHROUGH).unwrap();
        config.set_max_stack_depth(2).unwrap();
        Ok(())
    }

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        match name.to_str() {
            Some(name) => {
                match self.directory.root.find_ino(parent).map(|parent| parent.find_name(name)).flatten() {
                    Some(matching_entry) => {
                        reply.entry(&TTL, &matching_entry.get_fileattr(), 0);
                    }
                    None => {
                        println!("Failed to find {name}, parent: {parent}");
                        reply.error(ENOENT);
                    }
                }
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        match self.directory.root.find_ino(ino) {
            Some(entry) => {
                reply.attr(&TTL, &entry.get_fileattr());
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        match self.directory.root.find_ino(ino) {
            Some(entry) => {
                println!("Reading {} from {offset} to {}", entry.name, offset as usize+size as usize);
                let time = Utc::now();
                let mut buffer = [0u8;1024*1024];
                let mut buffer_part = &mut buffer[0..size as usize];
                match entry.full_path.read(&mut buffer_part, offset as usize) {
                    Ok(s) => {
                        reply.data(&buffer[0..s]);
                    }
                    Err(_) => {
                        reply.error(ENOENT);
                    }
                }
                let event = Event {
                    time,
                    event: EventType::Read(ReadEvent {
                        file: entry.name.clone(),
                        offset: offset as usize,
                        size: size as usize
                    })
                };
                self.event_sender.blocking_send(event);
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }
    
    fn create(
            &mut self,
            _req: &Request<'_>,
            parent: u64,
            name: &OsStr,
            _mode: u32,
            _umask: u32,
            _flags: i32,
            reply: fuser::ReplyCreate,
        ) {
        match name.to_str() {
            Some(name) => {
                println!("Creating file {name}");
                match self.directory.create_file(parent, name) {
                    Ok(entry) => {
                        reply.created(&TTL, &entry.get_fileattr(), 0, 0, 0);
                    }
                    Err(_) => {
                        reply.error(ENOENT);
                    }
                }
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    fn write(
            &mut self,
            _req: &Request<'_>,
            _ino: u64,
            _fh: u64,
            _offset: i64,
            _data: &[u8],
            _write_flags: u32,
            _flags: i32,
            _lock_owner: Option<u64>,
            _reply: fuser::ReplyWrite,
        ) {
        // ignoring writes. Doesn't seem to be necessary. If it becomes necessary for functionality, generate a new entry and hold the contents in memory
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        match self.directory.root.find_ino(ino) {
            Some(entry) => {
                match &entry.info {
                    EntryInfo::Directory(dir_entries) => {
                        let mut entries : Vec<_> = vec![
                            (1, FileType::Directory, "."),
                            (1, FileType::Directory, ".."),
                        ];
                        let mut fs_entries: Vec<_> = dir_entries.iter().map(|e| {
                            let ftype = match e.info {
                                EntryInfo::Directory(_) => FileType::Directory,
                                EntryInfo::File(_) => FileType::RegularFile
                            };
                            (e.inode, ftype, &e.name as &str)
                        }).collect();
                        entries.append(&mut fs_entries);

                        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
                            // i + 1 means the index of the next entry
                            if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                                break;
                            }
                        }
                        reply.ok();
                    }
                    EntryInfo::File(_) => {
                        reply.error(ENOENT);
                    }
                }
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }
}
