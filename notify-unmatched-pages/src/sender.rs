use lettre::message::Mailbox;

pub struct Sender {
    pub from: Mailbox,
}

impl Sender {
    pub fn new(from: Mailbox) -> Self {
        Self { from }
    }

    pub fn from(&self) -> &Mailbox {
        &self.from
    }
}
