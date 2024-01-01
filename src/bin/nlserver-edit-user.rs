use std::fs;

use anyhow::{Result, anyhow};
use dialoguer::Confirm;

use neolith::server::user_editor::InteractiveUserEditor;

fn main() -> Result<()> {
    let mut args = std::env::args();

    let filename = if let Some(filename) = args.nth(1) {
        Ok(filename)
    } else {
        Err(anyhow!("provide input filename"))
    }?;

    let input = fs::read(&filename)?;
    let output = InteractiveUserEditor::deserialize(&input)?
        .interact()?
        .serialize()?;

    let write = Confirm::new()
        .with_prompt("Are you sure you want to modify this user?")
        .interact()?;
    if write {
        fs::write(&filename, output)?;
        eprintln!("updated");
    } else {
        eprintln!("cancelled");
    }

    Ok(())
}
