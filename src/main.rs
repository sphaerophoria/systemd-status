mod systemd;

use std::{fs::OpenOptions, io::Write, time::Duration};

use anyhow::{Context, Result};
use log::error;
use ksni::{menu::StandardItem, MenuItem, Tray as KsniTray, TrayService};
use systemd::OrgFreedesktopSystemd1Manager;
use tempfile::TempDir;

#[derive(Clone, Debug)]
#[allow(dead_code)]
/// https://www.freedesktop.org/wiki/Software/systemd/dbus/
struct UnitInfo {
    name: String,
    description: String,
    load_state: String,
    active_state: String,
    sub_state: String,
    following_unit: String,
    object_path: dbus::Path<'static>,
    job_queued: u32,
    job_type: String,
    job_object_path: dbus::Path<'static>,
}

type DbusUnitInfoRet = (
    String,
    String,
    String,
    String,
    String,
    String,
    dbus::Path<'static>,
    u32,
    String,
    dbus::Path<'static>,
);

impl From<DbusUnitInfoRet> for UnitInfo {
    fn from(x: DbusUnitInfoRet) -> UnitInfo {
        UnitInfo {
            name: x.0,
            description: x.1,
            load_state: x.2,
            active_state: x.3,
            sub_state: x.4,
            following_unit: x.5,
            object_path: x.6,
            job_queued: x.7,
            job_type: x.8,
            job_object_path: x.9,
        }
    }
}

struct Tray {
    icon_dir: TempDir,
    stale: bool,
    failed_units: Vec<UnitInfo>,
}

impl Tray {
    fn new() -> Result<Tray> {
        let icon_dir = tempfile::tempdir()
            .context("Failed to create temporary directory for icons")?;

        let to_write = &[
            (include_bytes!("../res/ok.png"), "ok.png"),
            (include_bytes!("../res/stale.png"), "stale.png"),
            (include_bytes!("../res/err.png"), "err.png"),
        ];

        for (data, path) in to_write {
            let mut f = OpenOptions::new()
                .write(true)
                .create(true)
                .open(icon_dir.path().join(path))
                .context("Failed to open icon")?;

            f.write_all(*data)
                .context("Failed to write icon")?;
        }

        let tray = Tray {
            icon_dir,
            stale: true,
            failed_units: Default::default(),
        };

        Ok(tray)
    }
}

impl KsniTray for Tray {
    fn id(&self) -> String {
        "com.micksayson.systemd-status".into()
    }

    fn icon_theme_path(&self) -> String {
        self.icon_dir.path().to_str().unwrap().into()
    }

    fn icon_name(&self) -> String {
        if self.stale {
            return "stale".into();
        }

        match self.failed_units.is_empty() {
            true => "ok".into(),
            false => "err".into(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Tray>> {
        let mut ret = Vec::new();

        for ui in &self.failed_units {
            let item = StandardItem {
                label: ui.name.clone(),
                ..Default::default()
            };
            ret.push(From::from(item));
        }

        ret
    }
}

fn main() -> Result<()> {
    let conn = dbus::blocking::Connection::new_system()
        .context("Failed to connect to system bus")?;

    let proxy = conn.with_proxy(
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        Duration::from_secs(1),
    );

    let tray = Tray::new()
        .context("Failed to create tray")?;
    let tray = TrayService::new(tray);
    let handle = tray.handle();
    tray.spawn();

    loop {
        match proxy.list_units_filtered(vec!["failed"]) {
            Ok(infos) => {
                let unit_infos = infos 
                    .into_iter()
                    .map(UnitInfo::from)
                    .collect::<Vec<_>>();

                handle.update(move |tray: &mut Tray| {
                    tray.failed_units = unit_infos.clone();
                    tray.stale = false;
                });
            },
            Err(e) => {
                error!("Failed to list units: {}", e);
                handle.update(|tray: &mut Tray| {
                    tray.stale = true;
                });
            }
        }

        std::thread::sleep(Duration::from_secs(60));
    }
}
