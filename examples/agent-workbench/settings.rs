use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};
use workbench_ui::AgentWorkbench;
use xrack_runtime::{Component, LocaleContext};

const COMPOSER_FILE: &str = "composer.txt";
const LANGUAGE_FILE: &str = "language.txt";

pub(super) fn read_text(directory: &Path, name: &str, limit: u64) -> io::Result<Option<String>> {
    let file = match File::open(directory.join(name)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut value = String::new();
    file.take(limit + 1).read_to_string(&mut value)?;
    if value.len() as u64 > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "设置文件过大"));
    }
    Ok(Some(value))
}

pub(super) fn write_text(directory: &Path, name: &str, value: &str) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(value.as_bytes())?;
    file.persist(directory.join(name))?;
    Ok(())
}

pub(super) fn attach_language(ui: &mut AgentWorkbench, directory: Option<&Path>) {
    let locale = LocaleContext::default();
    if let Some(directory) = directory {
        match read_text(directory, LANGUAGE_FILE, 32) {
            Ok(Some(language)) => {
                if let Err(error) = locale.set_locale(language.trim()) {
                    ui.set_language_status(error);
                }
            }
            Ok(None) => {}
            Err(error) => ui.set_language_status(error.to_string()),
        }
    }
    ui.set_locale_context(locale.clone());

    let weak = ui.as_weak();
    let directory = directory.map(Path::to_owned);
    ui.on_select_language(move |language| {
        let Some(mut ui) = weak.upgrade() else { return };
        if !matches!(language.as_str(), "zh-CN" | "en-US") {
            return;
        }
        let result = locale.set_locale(&language).and_then(|()| {
            directory.as_deref().map_or(Ok(()), |directory| {
                write_text(directory, LANGUAGE_FILE, &format!("{language}\n"))
                    .map_err(|error| error.to_string())
            })
        });
        ui.set_language_status(result.err().unwrap_or_default());
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ComposerSettings {
    enter: &'static str,
    steer: bool,
}

impl Default for ComposerSettings {
    fn default() -> Self {
        Self {
            enter: "modified",
            steer: false,
        }
    }
}

impl ComposerSettings {
    fn from_values(enter: &str, steer: bool) -> io::Result<Self> {
        let enter = match enter {
            "modified" => "modified",
            "single-line" => "single-line",
            "enter" => "enter",
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "无效的发送键设置",
                ));
            }
        };
        Ok(Self { enter, steer })
    }

    fn load(directory: &Path) -> io::Result<Self> {
        let Some(value) = read_text(directory, COMPOSER_FILE, 64)? else {
            return Ok(Self::default());
        };
        let mut fields = value.split_whitespace();
        let (Some(enter), Some(followup), None) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "无效的发送设置"));
        };
        let steer = match followup {
            "queue" => false,
            "steer" => true,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "无效的追加设置")),
        };
        Self::from_values(enter, steer)
    }

    fn save(self, directory: &Path) -> io::Result<()> {
        let followup = if self.steer { "steer" } else { "queue" };
        write_text(
            directory,
            COMPOSER_FILE,
            &format!("{} {followup}\n", self.enter),
        )
    }
}

pub(super) fn attach_composer(ui: &mut AgentWorkbench, directory: Option<&Path>) {
    if let Some(directory) = directory {
        match ComposerSettings::load(directory) {
            Ok(settings) => {
                ui.set_composer_enter_mode(settings.enter.into());
                ui.set_composer_steers(settings.steer);
            }
            Err(error) => ui.set_composer_settings_status(format!("无法读取发送设置（{error}）")),
        }
    }
    let weak = ui.as_weak();
    let directory = directory.map(Path::to_owned);
    ui.on_save_composer_settings(move || {
        let (Some(mut ui), Some(directory)) = (weak.upgrade(), directory.as_deref()) else {
            return;
        };
        let result =
            ComposerSettings::from_values(&ui.get_composer_enter_mode(), ui.get_composer_steers())
                .and_then(|settings| settings.save(directory));
        ui.set_composer_settings_status(
            result
                .err()
                .map_or_else(String::new, |error| format!("无法保存发送设置（{error}）")),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_the_selected_interface_language() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let mut ui = AgentWorkbench::new();
        attach_language(&mut ui, Some(directory.path()));
        assert_eq!(ui.get_ui_language(), "zh-CN");
        ui.set_draft("continue editing".into());
        ui.invoke_select_language("en-US".into());
        assert_eq!(ui.get_ui_language(), "en-US");
        assert_eq!(ui.get_draft(), "continue editing");

        let mut reopened = AgentWorkbench::new();
        attach_language(&mut reopened, Some(directory.path()));
        assert_eq!(reopened.get_ui_language(), "en-US");
        reopened.invoke_select_language("zh-CN".into());
        assert_eq!(
            read_text(directory.path(), LANGUAGE_FILE, 32)?.as_deref(),
            Some("zh-CN\n")
        );
        Ok(())
    }

    #[test]
    fn persists_both_settings_and_restores_them_in_a_new_window() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let mut ui = AgentWorkbench::new();
        attach_composer(&mut ui, Some(directory.path()));
        assert_eq!(ui.get_composer_enter_mode(), "modified");
        assert!(!ui.get_composer_steers());
        ui.set_composer_enter_mode("single-line".into());
        ui.set_composer_steers(true);
        ui.invoke_save_composer_settings();

        let mut reopened = AgentWorkbench::new();
        attach_composer(&mut reopened, Some(directory.path()));
        assert_eq!(reopened.get_composer_enter_mode(), "single-line");
        assert!(reopened.get_composer_steers());
        Ok(())
    }

    #[test]
    fn an_invalid_file_uses_defaults_and_can_be_repaired() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        fs::write(directory.path().join(COMPOSER_FILE), "wrong queue\n")?;
        let mut ui = AgentWorkbench::new();
        attach_composer(&mut ui, Some(directory.path()));
        assert_eq!(ui.get_composer_enter_mode(), "modified");
        assert!(!ui.get_composer_steers());
        assert!(!ui.get_composer_settings_status().is_empty());
        ui.set_composer_enter_mode("enter".into());
        ui.invoke_save_composer_settings();
        assert!(ui.get_composer_settings_status().is_empty());
        assert_eq!(ComposerSettings::load(directory.path())?.enter, "enter");
        Ok(())
    }
}
