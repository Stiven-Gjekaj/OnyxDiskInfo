//! The glue between the window and the drives.
//!
//! The drives are read on a thread of their own, so the window never waits
//! for a drive. Each answer comes back to the thread of the window, which
//! keeps the views and shows the one that the person picked.

use std::cell::RefCell;

use onyx_core::{DriveInfo, in_list_order};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};

use crate::view::{self, DriveView};
use crate::{AppData, AttributeRow, DriveTab, InfoRow, Logic, MainWindow, OnyxTray};

thread_local! {
    /// The view of each drive, in the order of `onyx list`. Only the thread
    /// of the window touches it.
    static VIEWS: RefCell<Vec<DriveView>> = const { RefCell::new(Vec::new()) };

    /// The icon in the tray, when the host has a tray.
    static TRAY: RefCell<Option<OnyxTray>> = const { RefCell::new(None) };

    /// The tabs. The window keeps one model and its rows change in place, so
    /// a tab that a person or a screen reader holds stays the same tab while
    /// the health of each drive arrives.
    static TABS: std::rc::Rc<VecModel<DriveTab>> = std::rc::Rc::new(VecModel::default());
}

fn verdict(v: view::Verdict) -> crate::Verdict {
    match v {
        view::Verdict::Good => crate::Verdict::Good,
        view::Verdict::Caution => crate::Verdict::Caution,
        view::Verdict::Bad => crate::Verdict::Bad,
        view::Verdict::Unknown => crate::Verdict::Unknown,
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

/// Put the tabs and the current drive into the window.
fn show(ui: &MainWindow) {
    let data = ui.global::<AppData<'_>>();
    VIEWS.with_borrow(|views| {
        TABS.with(|tabs| {
            let wanted: Vec<DriveTab> = views
                .iter()
                .map(|v| DriveTab {
                    title: v.name.clone().into(),
                    temperature: v.temperature.clone().into(),
                    verdict: verdict(v.verdict),
                    verdict_text: v.verdict_text.clone().into(),
                })
                .collect();
            if tabs.row_count() == wanted.len() {
                for (index, tab) in wanted.into_iter().enumerate() {
                    if tabs.row_data(index).as_ref() != Some(&tab) {
                        tabs.set_row_data(index, tab);
                    }
                }
            } else {
                tabs.set_vec(wanted);
            }
            data.set_tabs(ModelRc::from(tabs.clone()));
        });
        let current = usize::try_from(data.get_current()).unwrap_or(0);
        let Some(v) = views.get(current).or(views.first()) else {
            data.set_name("No drive found".into());
            return;
        };
        let size = v
            .info
            .iter()
            .find(|(label, _)| label == "Size")
            .and_then(|(_, value)| value.split(" (").next())
            .unwrap_or("");
        data.set_name(format!("{}  {size}", v.name).into());
        data.set_verdict(verdict(v.verdict));
        data.set_verdict_text(v.verdict_text.clone().into());
        data.set_confidence(v.confidence.clone().into());
        data.set_source(v.source.clone().into());
        data.set_temperature(v.temperature.clone().into());
        data.set_temperature_verdict(verdict(v.temperature_verdict));
        data.set_info(model(
            v.info
                .iter()
                .map(|(label, value)| InfoRow {
                    label: label.clone().into(),
                    value: value.clone().into(),
                })
                .collect(),
        ));
        data.set_reasons(model(v.reasons.iter().map(SharedString::from).collect()));
        data.set_rows(model(
            v.rows
                .iter()
                .map(|r| AttributeRow {
                    id: r.id.clone().into(),
                    name: r.name.clone().into(),
                    current: r.current.clone().into(),
                    worst: r.worst.clone().into(),
                    threshold: r.threshold.clone().into(),
                    raw: r.raw.clone().into(),
                    warn: r.warn,
                })
                .collect(),
        ));
        data.set_message(v.message.clone().into());
        data.set_needs_rights(v.needs_rights);
        data.set_trend_commands(v.trend_commands.clone().into());
        data.set_trend_text(v.trend_text.clone().into());
        data.set_alerts(model(v.alerts.iter().map(SharedString::from).collect()));
    });
}

/// Change the view of one drive, and show the window again.
fn update(weak: &Weak<MainWindow>, index: usize, change: impl FnOnce(DriveView) -> DriveView) {
    VIEWS.with_borrow_mut(|views| {
        if let Some(slot) = views.get_mut(index) {
            *slot = change(slot.clone());
        }
    });
    if let Some(ui) = weak.upgrade() {
        show(&ui);
    }
}

/// The JSON of one drive, as `onyx list --json` gives it.
fn listed_json(number: usize, drive: &DriveInfo) -> Option<Value> {
    serde_json::from_str(&onyx::format::drive_json(number, drive).to_string()).ok()
}

/// The most alerts of one drive that the window shows.
const ALERTS_SHOWN: usize = 4;

/// The temperature of a drive over time, and its newest alerts as text.
fn history_of(store: &onyx_store::Store, key: &str) -> (Vec<(i64, i64)>, Vec<String>) {
    let temperature = store.series(key, "temperature").unwrap_or_default();
    let alerts = store
        .alerts_of(key, ALERTS_SHOWN)
        .unwrap_or_default()
        .iter()
        .map(|a| format!("{}: {}", onyx::history::utc(a.taken_at), a.alert.message))
        .collect();
    (temperature, alerts)
}

/// Read the list, then the health of each drive, on a thread of their own.
fn load(weak: Weak<MainWindow>) {
    if let Some(ui) = weak.upgrade() {
        ui.global::<AppData<'_>>().set_busy(true);
    }
    std::thread::spawn(move || {
        let drives = match onyx_probe::drive_list().and_then(|list| list.drives()) {
            Ok(drives) => in_list_order(drives),
            Err(e) => {
                let message = e.to_string();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    VIEWS.with_borrow_mut(Vec::clear);
                    let data = ui.global::<AppData<'_>>();
                    data.set_message(message.into());
                    data.set_busy(false);
                    show(&ui);
                });
                return;
            }
        };
        let listed: Vec<DriveView> = drives
            .iter()
            .enumerate()
            .filter_map(|(at, d)| listed_json(at + 1, d))
            .map(|json| view::listed(&json))
            .collect();
        let _ = weak.upgrade_in_event_loop(move |ui| {
            VIEWS.with_borrow_mut(|views| *views = listed);
            show(&ui);
        });

        // The history is a help and not a need: the app shows the health
        // without it when the file cannot open.
        let mut store = onyx::history::open(&onyx::cli::StoreArgs {
            db: None,
            json: false,
        })
        .ok();
        let taken_at = onyx::history::now();
        let mut new_alerts = 0;
        for (index, drive) in drives.iter().enumerate() {
            let record = onyx_probe::health_record(drive);
            let key = onyx::history::drive_of(drive);
            let (temperature, alerts) = match store.as_mut() {
                Some(store) => {
                    if let Ok(record) = &record {
                        let snapshot = onyx_core::snapshot::snapshot(record);
                        new_alerts += store.add(&key, taken_at, &snapshot).map_or(0, |a| a.len());
                    }
                    history_of(store, &key.key)
                }
                None => (Vec::new(), Vec::new()),
            };
            let answer = match record {
                Ok(record) => {
                    serde_json::from_str::<Value>(&onyx::report::json(drive, &record).to_string())
                        .map_err(|e| (e.to_string(), false))
                }
                Err(e) => Err(view::error_message(&e)),
            };
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                update(&weak, index, |v| {
                    let v = match &answer {
                        Ok(json) => view::with_health(v, json),
                        Err((message, rights)) => view::with_error(v, message, *rights),
                    };
                    view::with_history(v, &temperature, alerts)
                });
            });
        }
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.global::<AppData<'_>>().set_busy(false);
            let summary = VIEWS.with_borrow(|views| tray_summary(views, new_alerts));
            TRAY.with_borrow(|tray| {
                if let Some(tray) = tray {
                    tray.set_summary(summary.into());
                }
            });
        });
    });
}

/// The words of the tray: the worst verdict of the drives, and the alerts
/// that the last reading gave.
pub fn tray_summary(views: &[DriveView], new_alerts: usize) -> String {
    let worst = views
        .iter()
        .map(|v| v.verdict)
        .filter(|v| *v != view::Verdict::Unknown)
        .max_by_key(|v| match v {
            view::Verdict::Good => 0,
            view::Verdict::Caution => 1,
            view::Verdict::Bad => 2,
            view::Verdict::Unknown => -1,
        });
    let drives = match views.len() {
        1 => "1 drive".to_string(),
        n => format!("{n} drives"),
    };
    let state = match worst {
        Some(view::Verdict::Bad) => "one is Bad",
        Some(view::Verdict::Caution) => "one needs caution",
        Some(_) => "all Good",
        None => "none read",
    };
    let alerts = match new_alerts {
        0 => String::new(),
        1 => ", 1 new alert".to_string(),
        n => format!(", {n} new alerts"),
    };
    format!("OnyxDiskInfo: {drives}, {state}{alerts}")
}

/// Read the current drive with administrator rights, on a thread of its own.
fn read_with_rights(weak: Weak<MainWindow>) {
    let Some(ui) = weak.upgrade() else { return };
    let index = usize::try_from(ui.global::<AppData<'_>>().get_current()).unwrap_or(0);
    let Some(number) = VIEWS.with_borrow(|views| views.get(index).map(|v| v.number)) else {
        return;
    };
    ui.global::<AppData<'_>>().set_busy(true);
    std::thread::spawn(move || {
        let answer = crate::elevate::read(number)
            .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()));
        let weak2 = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            update(&weak2, index, |v| match &answer {
                Ok(json) => view::with_health(v, json),
                Err(message) => view::with_error(v, message, true),
            });
            if let Some(ui) = weak2.upgrade() {
                ui.global::<AppData<'_>>().set_busy(false);
            }
        });
    });
}

/// How often the app reads the drives while it runs.
const POLL: std::time::Duration = std::time::Duration::from_secs(3600);

/// Make the icon in the tray, when the host has one. A test with no display
/// has none, and `ONYX_NO_TRAY` turns it off.
fn tray(ui: &MainWindow) -> Option<OnyxTray> {
    let headless = std::env::var("SLINT_BACKEND").is_ok_and(|b| b == "headless");
    if headless || std::env::var_os("ONYX_NO_TRAY").is_some() {
        return None;
    }
    let tray = OnyxTray::new().ok()?;
    let weak = ui.as_weak();
    tray.on_show_window(move || {
        if let Some(ui) = weak.upgrade() {
            let _ = ui.show();
        }
    });
    let weak = ui.as_weak();
    tray.on_read_now(move || load(weak.clone()));
    tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
    tray.show().ok()?;
    Some(tray)
}

/// Open the window, and run until the person quits. With a tray, a closed
/// window hides, and the app reads the drives each hour.
pub fn run() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let logic = ui.global::<Logic<'_>>();

    let weak = ui.as_weak();
    logic.on_select(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.global::<AppData<'_>>().set_current(index);
            show(&ui);
        }
    });
    let weak = ui.as_weak();
    logic.on_refresh(move || load(weak.clone()));
    let weak = ui.as_weak();
    logic.on_read_with_rights(move || read_with_rights(weak.clone()));

    load(ui.as_weak());
    let Some(tray) = tray(&ui) else {
        return ui.run();
    };
    TRAY.with_borrow_mut(|slot| *slot = Some(tray));
    let poll = slint::Timer::default();
    let weak = ui.as_weak();
    poll.start(slint::TimerMode::Repeated, POLL, move || load(weak.clone()));
    ui.window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
    ui.show()?;
    slint::run_event_loop_until_quit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(verdict: view::Verdict) -> DriveView {
        let mut v = view::listed(&serde_json::json!({"number": 1, "name": "disk0"}));
        v.verdict = verdict;
        v
    }

    #[test]
    fn the_tray_names_the_worst_drive_and_the_new_alerts() {
        use view::Verdict::*;
        assert_eq!(
            tray_summary(&[drive(Good), drive(Good)], 0),
            "OnyxDiskInfo: 2 drives, all Good"
        );
        assert_eq!(
            tray_summary(&[drive(Good), drive(Caution)], 1),
            "OnyxDiskInfo: 2 drives, one needs caution, 1 new alert"
        );
        assert_eq!(
            tray_summary(&[drive(Bad), drive(Unknown)], 3),
            "OnyxDiskInfo: 2 drives, one is Bad, 3 new alerts"
        );
        assert_eq!(
            tray_summary(&[drive(Unknown)], 0),
            "OnyxDiskInfo: 1 drive, none read"
        );
    }
}
