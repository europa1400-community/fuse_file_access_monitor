use fuser::BackgroundSession;
use iced::alignment::Horizontal;
use iced::futures::SinkExt;
use iced::widget::text_input::Catalog;
use iced::{keyboard, Background, Border, Color, Theme};
use iced::widget::{
    self, button, center, checkbox, column, container, keyed_column, row, scrollable, text, text_editor, text_input, Column, Container, Text, TextInput
};
use iced::{Center, Element, Fill, Font, Subscription, Task as Command};
use tokio::sync::Mutex;
use std::sync::Arc;

use crate::fs::Event;

#[derive(Debug)]
pub struct AccessTrackingFsGui {
    state: State,
    event_sender : tokio::sync::mpsc::Sender<Event>,
    event_receiver : Arc<Mutex<tokio::sync::mpsc::Receiver<Event>>>,
}

impl Default for AccessTrackingFsGui {
    fn default() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(10000);
        Self {
            state: State::default(),
            event_sender: sender,
            event_receiver: Arc::new(Mutex::new(receiver))
        }
    }
}

#[derive(Debug)]
pub enum Status {
    Unmounting,
    Unmounted,
    Mounting,
    Mounted(BackgroundSession),
}

#[derive(Debug)]
struct State {
    pub source: String,
    pub mountpoint: String,
    pub source_valid: bool,
    pub mountpoint_valid: bool,
    pub status : Status,
    pub error_text : Option<String>,
    pub event_log : Vec<Event>,
    pub event_text : String,
    pub event_log_content: iced::widget::text_editor::Content
}


impl Default for State {
    fn default() -> Self {
        Self {
            source: String::new(),
            mountpoint: String::new(),
            source_valid: false,
            mountpoint_valid: false,
            status: Status::Unmounted,
            error_text: None,
            event_log: Vec::new(),
            event_text: String::new(),
            event_log_content: iced::widget::text_editor::Content::new()
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    UpdateSource(String),
    UpdateMountpoint(String),
    MountPressed,
    UnmountPressed,
    ReceivedEvent(Event),
    InitEventCommunication(tokio::sync::mpsc::Sender<Arc<Mutex<tokio::sync::mpsc::Receiver<Event>>>>),
    LogEdit(iced::widget::text_editor::Action)
}

impl AccessTrackingFsGui {
    /*
    pub fn new() -> (Self, Command<Message>) {
        (
            Self::Loading,
            Command::perform(SavedState::load(), Message::Loaded),
        )
    }
    */

    pub fn title(&self) -> String {
        format!("FUSE File Access Tracker")
    }

    pub fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::MountPressed => {
                self.state.mountpoint_valid = std::path::PathBuf::from(self.state.mountpoint.clone()).is_dir();
                self.state.source_valid = std::path::PathBuf::from(self.state.source.clone()).is_dir();
                if self.state.mountpoint_valid && self.state.source_valid {
                    self.state.status = Status::Mounting;
                    match super::run_mount(&self.state.source, &self.state.mountpoint, self.event_sender.clone()) {
                        Ok(process) => {
                            self.state.status = Status::Mounted(process);
                        }
                        Err(err) => {
                            self.state.error_text = Some(format!("{err}"));
                            self.state.status = Status::Unmounted;
                        }
                    }
                } else {
                    if !self.state.mountpoint_valid {
                        self.state.error_text = Some(format!("Mountpoint is not a directory."));
                    }
                    if !self.state.source_valid {
                        self.state.error_text = Some(format!("Source is not a directory."));
                    }
                }
            }
            Message::UnmountPressed => {
                let mut status = Status::Unmounting;
                std::mem::swap(&mut self.state.status, &mut status);
                match status {
                    Status::Mounted(process) => {
                        process.join();
                    }
                    _ => {
                        self.state.error_text = Some(format!("Somehow unmount was pressed, even though nothing was mounted...? Oh well."));
                    }
                }
                self.state.status = Status::Unmounted;
            }
            Message::UpdateMountpoint(path) => {
                self.state.mountpoint_valid = std::path::PathBuf::from(path.clone()).is_dir();
                self.state.mountpoint = path;
            }
            Message::UpdateSource(path) => {
                self.state.source_valid = std::path::PathBuf::from(path.clone()).is_dir();
                self.state.source = path;
            }
            Message::ReceivedEvent(event) => {
                self.state.event_log.push(event.clone());
                self.state.event_text.push_str(&format!("{event}\n"));
                self.state.event_log_content = iced::widget::text_editor::Content::with_text(&self.state.event_text)
            }
            Message::InitEventCommunication(sender) => {
                if sender.blocking_send(self.event_receiver.clone()).is_err() {
                    panic!("Failed to establish event communication! :3");
                }
            }
            Message::LogEdit(action) => {
                //action
                self.state.event_log_content.perform(action);
            }
        }
        Command::none()
    }

    fn directory_selector<'a>(placeholder: &str, text : &str, on_input: impl Fn(String) -> Message + 'a) -> TextInput<'a, Message> {
        text_input(placeholder, text)
            //.style(|theme, status| iced::widget::text_input::Style::(theme, status))
            .on_input(on_input)
            .width(400)
    }

    fn view_mounted(&self) -> Container<Message> {
        let centered_container = container(
            column![
                button("Unmount").on_press(Message::UnmountPressed),
                text(format!("{} events logged.", self.state.event_log.len())),
                scrollable(text_editor(&self.state.event_log_content).on_action(Message::LogEdit)),
            ]
        );

        container(centered_container)
            .width(iced::Fill)
            .height(iced::Fill)
            .align_x(Center)
            .align_y(Center)
    }

    fn view_unmounted(&self) -> Container<Message> {
        let centered_container = container(
            container(column![
                row![
                    text("Source Directory:").width(200).align_x(Horizontal::Right),
                    Self::directory_selector("Source Directory", &self.state.source, Message::UpdateSource).width(400),
                ].spacing(10).align_y(Center),
                row![
                    text("Mountpoint:").width(200).align_x(Horizontal::Right),
                    Self::directory_selector("Mountpoint", &self.state.mountpoint, Message::UpdateMountpoint).width(400),
                ].spacing(10).align_y(Center),
                iced::widget::Space::new(0, 30),
                button("Mount").on_press(Message::MountPressed)
            ].spacing(10).align_x(Center))
                .padding(10)
                .center(800)
                .align_x(Center)
        ).align_x(Center).align_y(Center);
        centered_container
    }

    pub fn view_loading(&self, display_text : &'static str) -> Container<Message> {
        container(
            text(display_text).align_x(Center).align_y(Center)
        ).width(iced::Fill).height(iced::Fill).align_x(Center).align_y(Center)
    }

    pub fn view(&self) -> Container<Message> {
        match self.state.status {
            Status::Unmounted => self.view_unmounted(),
            Status::Mounting => self.view_loading("Mounting..."),
            Status::Unmounting => self.view_loading("Unmounting..."),
            Status::Mounted(_) => self.view_mounted()
            
        }
    }

    fn some_worker() -> impl iced::futures::Stream<Item = Message> {
        iced::stream::channel(100, |mut output| async move {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(5);
            output.send(Message::InitEventCommunication(sender)).await;
            match receiver.recv().await {
                Some(receiver) => {
                    println!("Established event communication.");
                    let mut receiver = receiver.lock().await;
                    loop {
                        match receiver.recv().await {
                            Some(event) => {
                                output.send(Message::ReceivedEvent(event)).await;
                            }
                            None => {
                                break;
                            }
                        }
                    }
                }
                None => {
                    panic!("Could not establish event communication! :3");
                }
            }
        })
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::run(Self::some_worker)
    }
}
