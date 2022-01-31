// SPDX-License-Identifier: GPL-3.0-only

use apps_container::AppsContainer;
use cosmic_plugin::*;
use dock_list::DockListType;
use dock_object::DockObject;
use gdk4::glib::SourceId;
use gio::DesktopAppInfo;
use gtk4::{glib, prelude::*};
use once_cell::sync::OnceCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{collections::BTreeMap, thread::JoinHandle};
use tokio::sync::mpsc;
use utils::{block_on, BoxedWindowList, Event, Item, DEST, PATH};
use zbus::Connection;

mod apps_container;
mod dock_item;
mod dock_list;
mod dock_object;
mod dock_popover;
mod utils;

const ID: &str = "com.system76.apps";

#[derive(Debug, Default)]
pub struct Apps {
    tx: OnceCell<mpsc::Sender<Event>>,
    event_handle: OnceCell<SourceId>,
    cached_window_list: Arc<Mutex<Vec<Item>>>,
    zbus_handle: OnceCell<JoinHandle<()>>,
    close_thread: Arc<Mutex<bool>>,
    apps_container: OnceCell<AppsContainer>,
}

impl Apps {
    fn spawn_zbus(&self) -> Connection {
        let connection = block_on(Connection::session()).unwrap();

        let sender = self.tx.get().unwrap().clone();
        let conn = connection.clone();
        let close_thread = Arc::clone(&self.close_thread);
        let cached_window_list = Arc::clone(&self.cached_window_list);
        let zbus_handle = std::thread::spawn(move || {
            block_on(async move {
                while !*close_thread.lock().unwrap() {
                    let m = conn
                        .call_method(Some(DEST), PATH, Some(DEST), "WindowList", &())
                        .await;
                    if let Ok(m) = m {
                        if let Ok(mut reply) = m.body::<Vec<Item>>() {
                            let mut cached_results = cached_window_list.as_ref().lock().unwrap();
                            reply.sort_by(|a, b| a.name.cmp(&b.name));

                            if cached_results.len() != reply.len()
                                || !reply.iter().zip(cached_results.iter()).fold(
                                    0,
                                    |acc, z: (&Item, &Item)| {
                                        let (a, b) = z;
                                        if a.name == b.name {
                                            acc + 1
                                        } else {
                                            acc
                                        }
                                    },
                                ) == cached_results.len()
                            {
                                cached_results.splice(.., reply);
                                let _ = sender.send(Event::WindowList).await;
                            }
                        }
                        glib::timeout_future(Duration::from_millis(100)).await;
                    }
                }
            })
        });

        self.zbus_handle.set(zbus_handle).unwrap();
        connection
    }
}

impl Plugin for Apps {
    fn css_provider(&self) -> gtk4::CssProvider {
        // Load the css file and add it to the provider
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(include_bytes!("style.css"));
        provider
    }

    fn on_plugin_unload(&mut self) {
        {
            let mut ct = self.close_thread.lock().unwrap();
            *ct = true;
        }
        self.zbus_handle.take().unwrap().join().unwrap();
        self.event_handle.take().unwrap().remove();
        futures::executor::block_on(self.tx.take().unwrap().closed());
        drop(self.apps_container.take().unwrap());
    }

    fn on_plugin_load(&mut self) {
        let (tx, mut rx) = mpsc::channel(100);
        self.tx.set(tx.clone()).unwrap();
        let zbus_conn = self.spawn_zbus();

        let apps_container = apps_container::AppsContainer::new(tx.clone());
        self.apps_container.set(apps_container.clone()).unwrap();

        let cached_results = Arc::clone(&self.cached_window_list);
        let event_handle = glib::MainContext::default().spawn_local(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    Event::Activate(e) => {
                        let _activate_window = zbus_conn
                            .call_method(Some(DEST), PATH, Some(DEST), "WindowFocus", &((e,)))
                            .await
                            .expect("Failed to focus selected window");
                    }
                    Event::Close(e) => {
                        let _activate_window = zbus_conn
                            .call_method(Some(DEST), PATH, Some(DEST), "WindowQuit", &((e,)))
                            .await
                            .expect("Failed to close selected window");
                    }
                    Event::Favorite((name, should_favorite)) => {
                        dbg!(&name);
                        dbg!(should_favorite);
                        let saved_app_model = apps_container.model(DockListType::Saved);
                        let active_app_model = apps_container.model(DockListType::Active);
                        if should_favorite {
                            let mut cur: u32 = 0;
                            let mut index: Option<u32> = None;
                            while let Some(item) = active_app_model.item(cur) {
                                if let Ok(cur_dock_object) = item.downcast::<DockObject>() {
                                    if cur_dock_object.get_path() == Some(name.clone()) {
                                        cur_dock_object.set_saved(true);
                                        index = Some(cur);
                                    }
                                }
                                cur += 1;
                            }
                            if let Some(index) = index {
                                let object = active_app_model.item(index).unwrap();
                                active_app_model.remove(index);
                                saved_app_model.append(&object);
                            }
                        } else {
                            let mut cur: u32 = 0;
                            let mut index: Option<u32> = None;
                            while let Some(item) = saved_app_model.item(cur) {
                                if let Ok(cur_dock_object) = item.downcast::<DockObject>() {
                                    if cur_dock_object.get_path() == Some(name.clone()) {
                                        cur_dock_object.set_saved(false);
                                        index = Some(cur);
                                    }
                                }
                                cur += 1;
                            }
                            if let Some(index) = index {
                                let object = saved_app_model.item(index).unwrap();
                                saved_app_model.remove(index);
                                active_app_model.append(&object);
                            }
                        }
                        let _ = tx.send(Event::RefreshFromCache).await;
                    }
                    Event::RefreshFromCache => {
                        // println!("refreshing model from cache");
                        let cached_results = cached_results.as_ref().lock().unwrap();
                        let stack_active = cached_results.iter().fold(
                            BTreeMap::new(),
                            |mut acc: BTreeMap<String, BoxedWindowList>, elem| {
                                if let Some(v) = acc.get_mut(&elem.description) {
                                    v.0.push(elem.clone());
                                } else {
                                    acc.insert(
                                        elem.description.clone(),
                                        BoxedWindowList(vec![elem.clone()]),
                                    );
                                }
                                acc
                            },
                        );
                        let mut stack_active: Vec<BoxedWindowList> =
                            stack_active.into_values().collect();

                        // update active app stacks for saved apps into the saved app model
                        // then put the rest in the active app model (which doesn't include saved apps)
                        let saved_app_model = apps_container.model(DockListType::Saved);

                        let mut saved_i: u32 = 0;
                        while let Some(item) = saved_app_model.item(saved_i) {
                            if let Ok(dock_obj) = item.downcast::<DockObject>() {
                                if let Some(cur_app_info) =
                                    dock_obj.property::<Option<DesktopAppInfo>>("appinfo")
                                {
                                    if let Some((i, _s)) = stack_active
                                        .iter()
                                        .enumerate()
                                        .find(|(_i, s)| s.0[0].description == cur_app_info.name())
                                    {
                                        // println!(
                                        //     "found active saved app {} at {}",
                                        //     _s.0[0].name, i
                                        // );
                                        let active = stack_active.remove(i);
                                        dock_obj.set_property("active", active.to_value());
                                        saved_app_model.items_changed(
                                            saved_i.try_into().unwrap(),
                                            0,
                                            0,
                                        );
                                    } else if let Some(_) = cached_results
                                        .iter()
                                        .find(|s| s.description == cur_app_info.name())
                                    {
                                        dock_obj.set_property(
                                            "active",
                                            BoxedWindowList(Vec::new()).to_value(),
                                        );
                                        saved_app_model.items_changed(
                                            saved_i.try_into().unwrap(),
                                            0,
                                            0,
                                        );
                                    }
                                }
                            }
                            saved_i += 1;
                        }

                        let active_app_model = apps_container.model(DockListType::Active);
                        let model_len = active_app_model.n_items();
                        let new_results: Vec<glib::Object> = stack_active
                            .into_iter()
                            .map(|v| DockObject::from_search_results(v).upcast())
                            .collect();
                        active_app_model.splice(0, model_len, &new_results[..]);
                    }
                    Event::WindowList => {
                        // sort to make comparison with cache easier
                        let results = cached_results.as_ref().lock().unwrap();

                        // build active app stacks for each app
                        let stack_active = results.iter().fold(
                            BTreeMap::new(),
                            |mut acc: BTreeMap<String, BoxedWindowList>, elem| {
                                if let Some(v) = acc.get_mut(&elem.description) {
                                    v.0.push(elem.clone());
                                } else {
                                    acc.insert(
                                        elem.description.clone(),
                                        BoxedWindowList(vec![elem.clone()]),
                                    );
                                }
                                acc
                            },
                        );
                        let mut stack_active: Vec<BoxedWindowList> =
                            stack_active.into_values().collect();

                        // update active app stacks for saved apps into the saved app model
                        // then put the rest in the active app model (which doesn't include saved apps)
                        let saved_app_model = apps_container.model(DockListType::Saved);

                        let mut saved_i: u32 = 0;
                        while let Some(item) = saved_app_model.item(saved_i) {
                            if let Ok(dock_obj) = item.downcast::<DockObject>() {
                                if let Some(cur_app_info) =
                                    dock_obj.property::<Option<DesktopAppInfo>>("appinfo")
                                {
                                    if let Some((i, _s)) = stack_active
                                        .iter()
                                        .enumerate()
                                        .find(|(_i, s)| s.0[0].description == cur_app_info.name())
                                    {
                                        // println!("found active saved app {} at {}", s.0[0].name, i);
                                        let active = stack_active.remove(i);
                                        dock_obj.set_property("active", active.to_value());
                                        saved_app_model.items_changed(
                                            saved_i.try_into().unwrap(),
                                            0,
                                            0,
                                        );
                                    } else if let Some(_) = results
                                        .iter()
                                        .find(|s| s.description == cur_app_info.name())
                                    {
                                        dock_obj.set_property(
                                            "active",
                                            BoxedWindowList(Vec::new()).to_value(),
                                        );
                                        saved_app_model.items_changed(
                                            saved_i.try_into().unwrap(),
                                            0,
                                            0,
                                        );
                                    }
                                }
                            }
                            saved_i += 1;
                        }

                        let active_app_model = apps_container.model(DockListType::Active);
                        let model_len = active_app_model.n_items();
                        let new_results: Vec<glib::Object> = stack_active
                            .into_iter()
                            .map(|v| DockObject::from_search_results(v).upcast())
                            .collect();
                        active_app_model.splice(0, model_len, &new_results[..]);
                    }
                }
            }
        });
        self.event_handle.set(event_handle).unwrap();
    }

    fn applet(&self) -> gtk4::Box {
        self.apps_container
            .get()
            .unwrap()
            .clone()
            .upcast::<gtk4::Box>()
    }

    fn set_size(&self, _size: Size) {}

    fn set_position(&self, position: Position) {
        self.apps_container.get().unwrap().set_position(position);
    }
}

declare_plugin!(Apps);
