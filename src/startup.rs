use anyhow::Result;
use windows::core::{w, PCWSTR};

use crate::utils::{
    create_scheduled_task, delete_scheduled_task, exist_scheduled_task, get_exe_path, RegKey,
};

const TASK_NAME: &str = "TabOrbit";
const LEGACY_TASK_NAME: &str = "WindowSwitcher";
const HKEY_RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const HKEY_NAME: PCWSTR = w!("TabOrbit");
const LEGACY_HKEY_NAME: PCWSTR = w!("Window Switcher");

#[derive(Default)]
pub struct Startup {
    pub is_admin: bool,
    pub is_enable: bool,
    pub exe_path: Vec<u16>,
}

impl Startup {
    pub fn init(is_admin: bool) -> Result<Self> {
        let exe_path = get_exe_path();
        let is_enable = if is_admin {
            exist_scheduled_task(TASK_NAME)? || exist_scheduled_task(LEGACY_TASK_NAME)?
        } else {
            reg_is_enable(&exe_path)? || reg_is_enable_with_name(&exe_path, LEGACY_HKEY_NAME)?
        };
        Ok(Self {
            is_admin,
            is_enable,
            exe_path,
        })
    }

    pub fn toggle(&mut self) -> Result<()> {
        match (self.is_admin, self.is_enable) {
            (true, true) => {
                if exist_scheduled_task(TASK_NAME)? {
                    delete_scheduled_task(TASK_NAME)?;
                }
                if exist_scheduled_task(LEGACY_TASK_NAME)? {
                    delete_scheduled_task(LEGACY_TASK_NAME)?;
                }
                self.is_enable = false;
            }
            (true, false) => {
                if reg_is_enable(&self.exe_path)? {
                    reg_disable()?;
                }
                if exist_scheduled_task(LEGACY_TASK_NAME)? {
                    delete_scheduled_task(LEGACY_TASK_NAME)?;
                }
                create_scheduled_task(TASK_NAME, &String::from_utf16_lossy(&self.exe_path))?;
                self.is_enable = true;
            }
            (false, true) => {
                if reg_is_enable(&self.exe_path)? {
                    reg_disable()?;
                }
                if reg_is_enable_with_name(&self.exe_path, LEGACY_HKEY_NAME)? {
                    reg_disable_with_name(LEGACY_HKEY_NAME)?;
                }
                self.is_enable = false;
            }
            (false, false) => {
                if exist_scheduled_task(TASK_NAME)? || exist_scheduled_task(LEGACY_TASK_NAME)? {
                    alert!("To avoid conflicts, please disable 'Startup' feature within TabOrbit while running it as an administrator. Once disabled, you can safely enable 'Startup' again under normal user permissions.");
                    return Ok(());
                }
                reg_enable(&self.exe_path)?;
                self.is_enable = true;
            }
        }
        Ok(())
    }
}

fn reg_key() -> Result<RegKey> {
    RegKey::new_hkcu(HKEY_RUN, HKEY_NAME)
}

fn reg_key_with_name(name: PCWSTR) -> Result<RegKey> {
    RegKey::new_hkcu(HKEY_RUN, name)
}

fn reg_is_enable(exe_path: &[u16]) -> Result<bool> {
    reg_is_enable_with_name(exe_path, HKEY_NAME)
}

fn reg_is_enable_with_name(exe_path: &[u16], name: PCWSTR) -> Result<bool> {
    let key = reg_key_with_name(name)?;
    let value = match key.get_value()? {
        Some(value) => value,
        None => return Ok(false),
    };
    Ok(value == exe_path)
}

fn reg_enable(exe_path: &[u16]) -> Result<()> {
    let key = reg_key()?;
    let path = unsafe { exe_path.align_to::<u8>().1 };
    key.set_value(path)?;
    Ok(())
}

fn reg_disable() -> Result<()> {
    reg_disable_with_name(HKEY_NAME)
}

fn reg_disable_with_name(name: PCWSTR) -> Result<()> {
    let key = reg_key_with_name(name)?;
    key.delete_value()?;
    Ok(())
}
