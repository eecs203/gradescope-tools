use std::env::{self, VarError};
use std::fmt;

pub struct Creds {
    email: String,
    password: String,
}

impl Creds {
    pub fn from_env() -> Result<Self, VarError> {
        let username = env::var("EMAIL")?;
        let password = env::var("GS_PASSWORD")?;
        Ok(Self::new(username, password))
    }

    pub fn new(email: String, password: String) -> Self {
        Self { email, password }
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for Creds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Creds")
            .field("email", &self.email)
            .field("password", &"<hidden>")
            .finish()
    }
}
