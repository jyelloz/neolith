use std::fmt::Display;

use anyhow::Result;
use dialoguer::{
    Input,
    Password,
    MultiSelect,
};
use strum::IntoEnumIterator;

use super::application::{
    UserAccount,
    UserDataFile,
    Permissions,
};

fn input_permissions<F, P>(prompt: &str, perms: &mut P) -> Result<()>
    where P: Permissions<F> + FromIterator<F>,
          F: Copy + IntoEnumIterator + Display {
    let items: Vec<_> = F::iter()
        .map(|op| (op, perms.can(op)))
        .collect();
    *perms = MultiSelect::new()
        .with_prompt(prompt)
        .items_checked(&items)
        .interact()?
        .into_iter()
        .map(|i| items[i].0)
        .collect();
    Ok(())
}

#[derive(Default)]
pub struct InteractiveUserEditor(UserAccount);
impl InteractiveUserEditor {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let s = std::str::from_utf8(input)?;
        let account: UserDataFile = toml::from_str(s)?;
        Ok(Self(account.into()))
    }

    fn input_identity(&mut self) -> Result<()> {
        let username_pattern = regex::Regex::new(r"^[a-z0-9_-]{1,32}$")?;
        fn byte_length(s: &str, min: usize, max: usize) -> bool {
            let len = s.as_bytes().len();
            min <= len && len <= max
        }

        let Self(account) = self;

        account.identity.login = Input::new()
            .with_prompt("Username")
            .validate_with(|s: &String| -> Result<(), String> {
                if username_pattern.is_match(s) {
                    Ok(())
                } else {
                    Err(format!("Invalid Username: must match regex {username_pattern}"))
                }
            })
            .default(account.identity.login.clone())
            .interact_text()?;

        account.identity.name = Input::new()
            .with_prompt("Nickname")
            .validate_with(|s: &String| -> Result<(), &str> {
                if byte_length(s, 1, 32) {
                    Ok(())
                } else {
                    Err("Invalid Nickname: length out of range 1..32")
                }
            })
        .default(account.identity.name.clone())
            .interact_text()?;

        account.identity.password = Password::new()
            .with_prompt("Password")
            .with_confirmation("Re-enter password", "password entry mismatch")
            .interact()?
            .try_into()?;
        Ok(())
    }

    pub fn interact(mut self) -> Result<Self> {
        self.input_identity()?;

        let Self(account) = &mut self;

        input_permissions("File Permissions", &mut account.permissions.file)?;
        input_permissions("User Permissions", &mut account.permissions.user)?;
        input_permissions("News Permissions", &mut account.permissions.news)?;
        input_permissions("Chat Permissions", &mut account.permissions.chat)?;
        input_permissions("Misc Permissions", &mut account.permissions.misc)?;

        Ok(self)
    }

    pub fn serialize(self) -> Result<String> {
        let Self(account) = self;
        let file: UserDataFile = account.into();
        let toml = toml::to_string(&file)?;
        Ok(toml)
    }
}
