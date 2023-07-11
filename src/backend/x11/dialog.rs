//! This module contains functions for opening file dialogs using DBus.

use std::path::PathBuf;

use ashpd::{desktop::file_chooser, WindowIdentifier};
use futures::executor::block_on;
use tracing::warn;

use crate::{FileDialogOptions, FileDialogToken, FileInfo};

use super::window::IdleHandle;

pub(crate) fn open_file(
    window: u32,
    idle: IdleHandle,
    options: FileDialogOptions,
) -> FileDialogToken {
    dialog(window, idle, options, true)
}

pub(crate) fn save_file(
    window: u32,
    idle: IdleHandle,
    options: FileDialogOptions,
) -> FileDialogToken {
    dialog(window, idle, options, false)
}

fn dialog(
    window: u32,
    idle: IdleHandle,
    mut options: FileDialogOptions,
    open: bool,
) -> FileDialogToken {
    let tok = FileDialogToken::next();

    std::thread::spawn(move || {
        if let Err(e) = block_on(async {
            let id = WindowIdentifier::from_xid(window as u64);
            let multi = options.multi_selection;

            let title_owned = options.title.take();
            let title = match (open, options.select_directories) {
                (true, true) => "Open Folder",
                (true, false) => "Open File",
                (false, _) => "Save File",
            };
            let title = title_owned.as_deref().unwrap_or(title);
            let response = if open {
                file_chooser::OpenFileRequest::default()
                    .identifier(id)
                    .title(title)
                    .modal(true)
                    .multiple(options.multi_selection)
                    .directory(options.select_directories)
                    .accept_label(options.button_text.as_deref())
                    .filters(
                        options
                            .allowed_types
                            .unwrap_or_default()
                            .into_iter()
                            .map(From::from),
                    )
                    .current_filter(options.default_type.map(From::from))
                    .send()
                    .await?
                    .response()?
            } else {
                file_chooser::SaveFileRequest::default()
                    .identifier(id)
                    .title(title)
                    .modal(true)
                    .current_name(options.default_name.as_deref())
                    .current_folder::<PathBuf>(options.starting_directory)?
                    .accept_label(options.button_text.as_deref())
                    .filters(
                        options
                            .allowed_types
                            .unwrap_or_default()
                            .into_iter()
                            .map(From::from),
                    )
                    .current_filter(options.default_type.map(From::from))
                    .send()
                    .await?
                    .response()?
            };
            let uris = response.uris();
            let mut paths = uris.iter().filter_map(|s| {
                s.to_file_path().ok().or_else(|| {
                    warn!("Invalid file path '{s}'");
                    None
                })
            });
            if multi && open {
                let infos = paths.map(|path| FileInfo { path, format: None }).collect();
                idle.add_idle_callback(move |handler| handler.open_files(tok, infos));
            } else if !multi {
                if uris.len() > 2 {
                    warn!(
                        "expected one path (got {}), returning only the first",
                        uris.len()
                    );
                }
                let info = paths.next().map(|path| FileInfo { path, format: None });
                if open {
                    idle.add_idle_callback(move |handler| handler.open_file(tok, info));
                } else {
                    idle.add_idle_callback(move |handler| handler.save_as(tok, info));
                }
            } else {
                warn!("cannot save multiple paths");
            }

            ashpd::Result::Ok(())
        }) {
            warn!("error while opening file dialog: {}", e);
        }
    });

    tok
}

impl From<crate::FileSpec> for file_chooser::FileFilter {
    fn from(spec: crate::FileSpec) -> file_chooser::FileFilter {
        let mut filter = file_chooser::FileFilter::new(spec.name);
        for ext in spec.extensions {
            filter = filter.glob(&format!("*.{ext}"));
        }
        filter
    }
}
