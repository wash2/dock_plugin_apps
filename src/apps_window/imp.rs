// SPDX-License-Identifier: MPL-2.0-only

use crate::apps_container::AppsContainer;
use gtk4::{glib, subclass::prelude::*};
use once_cell::sync::OnceCell;
// Object holding the state
#[derive(Default)]

pub struct CosmicDockAppListWindow {
    pub(super) inner: OnceCell<AppsContainer>,
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for CosmicDockAppListWindow {
    // `NAME` needs to match `class` attribute of template
    const NAME: &'static str = "CosmicDockAppListWindow";
    type Type = super::CosmicDockAppListWindow;
    type ParentType = gtk4::ApplicationWindow;
}

// Trait shared by all GObjects
impl ObjectImpl for CosmicDockAppListWindow {}

// Trait shared by all widgets
impl WidgetImpl for CosmicDockAppListWindow {}

// Trait shared by all windows
impl WindowImpl for CosmicDockAppListWindow {}

// Trait shared by all application
impl ApplicationWindowImpl for CosmicDockAppListWindow {}
