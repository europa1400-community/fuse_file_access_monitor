pub mod fs;
pub mod ui;

use fuser::{BackgroundSession, MountOption};

use fs::Event;

pub fn run_mount(mount_source : &str, mount_point : &str, event_sender : tokio::sync::mpsc::Sender<Event>) -> Result<BackgroundSession, std::io::Error> {
    let options = vec![MountOption::FSName("passthrough".to_string())];
    let fs = fs::FileAccessTrackingFs::new(mount_source, event_sender);
    fuser::spawn_mount2(fs, mount_point, &options)
}
