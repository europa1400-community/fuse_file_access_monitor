use fuse_file_access_monitor::ui::*;

fn main() -> iced::Result {
    iced::application("FUSE File Access Monitor", AccessTrackingFsGui::update, AccessTrackingFsGui::view)
        .subscription(AccessTrackingFsGui::subscription)
        .centered()
        .window_size((800.0, 600.0))
        .run()
}
