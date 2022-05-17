// SPDX-License-Identifier: MPL-2.0-only

use crate::{apps_container::AppsContainer, fl, Event};
use cascade::cascade;
use gtk4::{
    gio,
    glib::{self, Object},
    prelude::*,
    subclass::prelude::*,
};
use tokio::sync::mpsc;

mod imp;

glib::wrapper! {
    pub struct CosmicDockAppListWindow(ObjectSubclass<imp::CosmicDockAppListWindow>)
        @extends gtk4::ApplicationWindow, gtk4::Window, gtk4::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk4::Accessible, gtk4::Buildable,
                    gtk4::ConstraintTarget, gtk4::Native, gtk4::Root, gtk4::ShortcutManager;
}

impl CosmicDockAppListWindow {
    pub fn new(app: &gtk4::Application, tx: mpsc::Sender<Event>) -> Self {
        let self_: Self = Object::new(&[("application", app)])
            .expect("Failed to create `CosmicDockAppListWindow`.");
        let imp = imp::CosmicDockAppListWindow::from_instance(&self_);

        cascade! {
            &self_;
            ..set_width_request(1);
            ..set_height_request(1);
            ..set_decorated(false);
            ..set_resizable(false);
            ..set_title(Some(&fl!("cosmic-dock-app-list")));
            ..add_css_class("root_window");
        };
        let app_list = AppsContainer::new(tx);
        self_.set_child(Some(&app_list));
        imp.inner.set(app_list).unwrap();

        self_.setup_shortcuts();

        self_
    }

    fn setup_shortcuts(&self) {
        let window = self.clone().upcast::<gtk4::Window>();
        let action_quit = gio::SimpleAction::new("quit", None);
        action_quit.connect_activate(glib::clone!(@weak window => move |_, _| {
            window.close();
            window.application().map(|a| a.quit());
            std::process::exit(0);
        }));
        self.add_action(&action_quit);
    }
}
