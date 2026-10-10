//! The glue between the window and the drives.
//!
//! The drives are read on a thread of their own, so the window never waits
//! for a drive. Each answer comes back to the thread of the window, which
//! keeps the views and shows the one that the person picked.

use std::cell::RefCell;

use onyx_core::{DriveInfo, in_list_order};
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};

use crate::view::{self, DriveView};
use crate::{AppData, AttributeRow, DriveTab, InfoRow, Logic, MainWindow};

thread_local! {
    /// The view of each drive, in the order of `onyx list`. Only the thread
    /// of the window touches it.
    static VIEWS: RefCell<Vec<DriveView>> = const { RefCell::new(Vec::new()) };
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
        data.set_tabs(model(
            views
                .iter()
                .map(|v| DriveTab {
                    title: v.name.clone().into(),
                    temperature: v.temperature.clone().into(),
                    verdict: verdict(v.verdict),
                    verdict_text: v.verdict_text.clone().into(),
                })
                .collect(),
        ));
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

        for (index, drive) in drives.iter().enumerate() {
            let answer = match onyx_probe::health_record(drive) {
                Ok(record) => {
                    serde_json::from_str::<Value>(&onyx::report::json(drive, &record).to_string())
                        .map_err(|e| (e.to_string(), false))
                }
                Err(e) => Err(view::error_message(&e)),
            };
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                update(&weak, index, |v| match &answer {
                    Ok(json) => view::with_health(v, json),
                    Err((message, rights)) => view::with_error(v, message, *rights),
                });
            });
        }
        let _ = weak.upgrade_in_event_loop(|ui| ui.global::<AppData<'_>>().set_busy(false));
    });
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

/// Open the window, and run until the person closes it.
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
    ui.run()
}
