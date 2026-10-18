// Prevent console window in addition to Slint window in Windows release builds when, e.g. starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod note;
mod store;

use std::cell::RefCell;
use std::error::Error;
use std::rc::Rc;

slint::include_modules!();

use app::{AppState, PendingAction};

fn main() -> Result<(), Box<dyn Error>> {
    let ui = MainWindow::new()?;

    let config = config::Config::load();
    let app = Rc::new(RefCell::new(AppState::new(config, ui.as_weak())));

    let a = app.clone();
    ui.on_use_folder(move |path: slint::SharedString| {
        a.borrow_mut().run_or_defer(PendingAction::UseFolder(path.to_string()));
    });
    let a = app.clone();
    ui.on_new_note(move |name: slint::SharedString| {
        a.borrow_mut().run_or_defer(PendingAction::NewNote(name.to_string()));
    });
    let a = app.clone();
    ui.on_delete_selected(move || {
        a.borrow_mut().run_or_defer(PendingAction::DeleteSelected);
    });
    let a = app.clone();
    ui.on_refresh(move || a.borrow_mut().run_or_defer(PendingAction::Refresh));
    let a = app.clone();
    ui.on_toggle_hide_done(move || a.borrow_mut().toggle_hide_done());
    let a = app.clone();
    ui.on_change_folder(move || a.borrow_mut().change_folder());
    let a = app.clone();
    ui.on_select(move |filename: slint::SharedString| {
        a.borrow_mut().run_or_defer(PendingAction::Select(filename.to_string()));
    });
    let a = app.clone();
    ui.on_toggle_done(move |filename: slint::SharedString| {
        a.borrow_mut().run_or_defer(PendingAction::ToggleDone(filename.to_string()));
    });
    let a = app.clone();
    ui.on_save_detail(move || a.borrow_mut().save_detail());
    let a = app.clone();
    ui.on_discard_confirmed(move || a.borrow_mut().discard_confirmed());
    let a = app.clone();
    ui.on_discard_cancelled(move || a.borrow_mut().discard_cancelled());

    app.borrow_mut().startup();

    ui.run()?;

    Ok(())
}
