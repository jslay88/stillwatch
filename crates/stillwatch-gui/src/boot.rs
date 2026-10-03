//! Opens a [`Shell`](crate::shell::Shell) with the config file loaded, when there is one.

use stillwatch_ipc::paths;

use crate::settings::Editor;
use crate::shell::Shell;

/// A shell on `presets`, with the settings form read from the config file.
///
/// A missing file leaves the defaults. A file that doesn't parse leaves the
/// defaults and an error, so a later save can replace it.
#[must_use]
pub fn shell(presets: Vec<u32>) -> Shell {
    let mut shell = Shell::new(presets);
    let Ok(path) = paths::config_file() else {
        return shell;
    };
    shell.config_path = Some(path.clone());
    match Editor::load(&path) {
        Ok(editor) => shell.editor = editor,
        Err(err) => {
            shell.editor.set_load_error(format!(
                "{err}. Showing defaults; saving replaces the file."
            ));
        }
    }
    shell
}
