//! This module contains functions for opening file dialogs using DBus.

use ashpd::desktop::file_chooser;
use ashpd::WindowIdentifier;
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
            let open_result;
            let save_result;
            let uris = if open {
                let open_builder = file_chooser::OpenFileRequest::default()
                    .identifier(id)
                    .title(title);
                open_result = options
                    .apply_to_open(open_builder)
                    .send()
                    .await?
                    .response()?;
                open_result.uris()
            } else {
                let save_builder = file_chooser::SaveFileRequest::default()
                    .identifier(id)
                    .title(title);
                save_result = options
                    .apply_to_save(save_builder)
                    .send()
                    .await?
                    .response()?;
                save_result.uris()
            };

            let mut paths = uris.iter().filter_map(|s| {
                s.as_str().strip_prefix("file://").or_else(|| {
                    warn!("expected path '{}' to start with 'file://'", s);
                    None
                })
            });
            if multi && open {
                let infos = paths
                    .map(|p| FileInfo {
                        path: p.into(),
                        format: None,
                    })
                    .collect();
                idle.add_idle_callback(move |handler| handler.open_files(tok, infos));
            } else if !multi {
                if uris.len() > 2 {
                    warn!(
                        "expected one path (got {}), returning only the first",
                        uris.len()
                    );
                }
                let info = paths.next().map(|p| FileInfo {
                    path: p.into(),
                    format: None,
                });
                if open {
                    idle.add_idle_callback(move |handler| handler.open_file(tok, info));
                } else {
                    idle.add_idle_callback(move |handler| handler.save_as(tok, info));
                }
            } else {
                warn!("cannot save multiple paths");
            }

            Ok(()) as ashpd::Result<()>
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

impl crate::FileDialogOptions {
    fn apply_to_open(self, mut fc: file_chooser::OpenFileRequest) -> file_chooser::OpenFileRequest {
        fc = fc
            .modal(true)
            .multiple(self.multi_selection)
            .directory(self.select_directories);

        if let Some(label) = self.button_text {
            fc = fc.accept_label(label.as_str());
        }

        if let Some(filters) = self.allowed_types {
            fc = fc.filters(filters.into_iter().map(Into::into));
        }

        if let Some(filter) = self.default_type {
            fc = fc.current_filter(Some(filter.into()));
        }

        fc
    }
}

impl crate::FileDialogOptions {
    fn apply_to_save(self, mut fc: file_chooser::SaveFileRequest) -> file_chooser::SaveFileRequest {
        fc = fc.modal(true);

        if let Some(name) = self.default_name {
            fc = fc.current_name(name.as_str());
        }

        if let Some(label) = self.button_text {
            fc = fc.accept_label(label.as_str());
        }

        if let Some(filters) = self.allowed_types {
            fc = fc.filters(filters.into_iter().map(Into::into));
        }

        if let Some(filter) = self.default_type {
            fc = fc.current_filter(Some(filter.into()));
        }

        if let Some(dir) = self.starting_directory {
            fc = fc
                .current_folder(dir)
                .expect("Shouldn't have nul bytes in provided directory");
        }

        fc
    }
}
