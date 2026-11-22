use crate::review::recolor_raw_diff;
use crate::settings::{read_text, write_text};
use std::{io, path::Path};
use workbench_ui::AgentWorkbench;
use xrack_runtime::ColorScheme;

const FILE_NAME: &str = "appearance.txt";

fn load(directory: &Path) -> io::Result<ColorScheme> {
    let Some(value) = read_text(directory, FILE_NAME, 32)? else {
        return Ok(ColorScheme::System);
    };
    match value.as_str() {
        "system\n" => Ok(ColorScheme::System),
        "light\n" => Ok(ColorScheme::Light),
        "dark\n" => Ok(ColorScheme::Dark),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "无效的外观设置")),
    }
}

fn save(directory: &Path, scheme: ColorScheme) -> io::Result<()> {
    let value = match scheme {
        ColorScheme::System => "system\n",
        ColorScheme::Light => "light\n",
        ColorScheme::Dark => "dark\n",
    };
    write_text(directory, FILE_NAME, value)
}

pub(super) fn attach(ui: &mut AgentWorkbench, directory: Option<&Path>) {
    if let Some(directory) = directory {
        match load(directory) {
            Ok(scheme) => ui.invoke_select_appearance(scheme),
            Err(error) => ui.set_appearance_status(format!("无法读取外观设置（{error}）")),
        }
    }

    let weak = ui.as_weak();
    let directory = directory.map(Path::to_owned);
    ui.on_save_appearance(move |scheme| {
        let Some(mut ui) = weak.upgrade() else { return };
        recolor_raw_diff(&mut ui);
        ui.invoke_refresh_chat_find();
        let status = directory
            .as_deref()
            .and_then(|directory| save(directory, scheme).err())
            .map_or_else(String::new, |error| format!("无法保存外观设置（{error}）"));
        ui.set_appearance_status(status);
    });
}

pub(super) fn update_system(ui: &mut AgentWorkbench, scheme: ColorScheme) {
    let dark = ui.get_dark_appearance();
    ui.invoke_system_appearance_changed(scheme);
    if dark != ui.get_dark_appearance() {
        recolor_raw_diff(ui);
        ui.invoke_refresh_chat_find();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn remembers_a_selection_and_reacts_to_the_host_only_in_system_mode() -> io::Result<()> {
        let state = tempfile::tempdir()?;
        let mut ui = AgentWorkbench::new();
        attach(&mut ui, Some(state.path()));
        assert_eq!(ui.get_appearance_mode(), ColorScheme::System);
        update_system(&mut ui, ColorScheme::Dark);
        assert!(ui.get_dark_appearance());

        ui.invoke_select_appearance(ColorScheme::Light);
        assert!(!ui.get_dark_appearance());
        assert_eq!(load(state.path())?, ColorScheme::Light);
        update_system(&mut ui, ColorScheme::Light);
        update_system(&mut ui, ColorScheme::Dark);
        assert!(!ui.get_dark_appearance());

        let mut reopened = AgentWorkbench::new();
        attach(&mut reopened, Some(state.path()));
        assert_eq!(reopened.get_appearance_mode(), ColorScheme::Light);
        update_system(&mut reopened, ColorScheme::Dark);
        reopened.invoke_select_appearance(ColorScheme::System);
        assert!(reopened.get_dark_appearance());
        assert_eq!(load(state.path())?, ColorScheme::System);
        Ok(())
    }

    #[test]
    fn unreadable_preferences_fall_back_and_can_be_replaced() -> io::Result<()> {
        let state = tempfile::tempdir()?;
        fs::write(state.path().join(FILE_NAME), "invalid")?;
        let mut ui = AgentWorkbench::new();
        attach(&mut ui, Some(state.path()));
        assert_eq!(ui.get_appearance_mode(), ColorScheme::System);
        assert!(!ui.get_appearance_status().is_empty());
        ui.invoke_select_appearance(ColorScheme::Dark);
        assert_eq!(load(state.path())?, ColorScheme::Dark);
        assert!(ui.get_appearance_status().is_empty());
        Ok(())
    }
}
