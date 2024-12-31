use anyhow::Result;

use neolith::server::user_editor::InteractiveUserEditor;

fn main() -> Result<()> {
    let file = InteractiveUserEditor::default().interact()?.serialize()?;

    print!("{}", file);

    Ok(())
}
