#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Locale {
    #[default]
    Zh,
    En,
}

pub enum Message {
    RemindPrefix,
    TaskDue { title: String },
}

pub fn tr(message: Message, _locale: Locale) -> String {
    match message {
        Message::RemindPrefix => "Reminder: ".to_string(),
        Message::TaskDue { title } => format!("Task due: {title}"),
    }
}
