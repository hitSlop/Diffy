use super::Diffy;
use crate::updates;
use gpui_kit::AnyElement;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::Styled as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::h_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use gpui_kit::rgb;
#[cfg(feature = "ui-tests")]
use std::sync::Arc;
use std::time::Duration;

impl Diffy {
    pub(super) fn check_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if !self.updates.begin_check() {
            return;
        }
        self.updates.show_check = manual;
        let backend = self.updates.backend.clone();
        self.update_task = Some(cx.spawn(async move |owner, cx| {
            let result = cx.background_spawn(async move { backend.check() }).await;
            let _ = owner.update(cx, |this, cx| {
                this.updates.checked(result, manual);
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn install_update(&mut self, cx: &mut Context<Self>) {
        if self.updates.busy() {
            return;
        }
        let Some(release) = self.updates.release.clone() else {
            return;
        };
        if let Some(error) = updates::installation_problem() {
            self.updates.phase = updates::Phase::Error(error);
            cx.notify();
            return;
        }
        self.persist(cx);
        if self.state_error.is_some() {
            self.updates.phase = updates::Phase::Error(
                "Couldn't save projects. Resolve the preference error before updating.".into(),
            );
            cx.notify();
            return;
        }
        self.stop_file_repeat();
        self.updates.phase = updates::Phase::Downloading {
            received: 0,
            total: None,
        };
        self.update_task = Some(cx.spawn(async move |owner, cx| {
            let (sender, receiver) = async_channel::bounded(1);
            let package = release.package.clone();
            let download = cx.background_spawn(async move {
                let received = std::sync::atomic::AtomicU64::new(0);
                package.download(&|chunk, total| {
                    let count = received
                        .fetch_add(chunk as u64, std::sync::atomic::Ordering::Relaxed)
                        + chunk as u64;
                    let _ = sender.try_send((count, total));
                })
            });
            while let Ok((received, total)) = receiver.recv().await {
                if owner
                    .update(cx, |this, cx| {
                        this.updates.phase = updates::Phase::Downloading { received, total };
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let bytes = match download.await {
                Ok(bytes) => bytes,
                Err(error) => {
                    let _ = owner.update(cx, |this, cx| {
                        this.updates.phase = updates::Phase::Error(error);
                        cx.notify();
                    });
                    return;
                }
            };
            loop {
                let waiting = owner.update(cx, |this, cx| {
                    let waiting = this.projects.iter().any(|p| p.mutating);
                    this.updates.phase = if waiting {
                        updates::Phase::WaitingForGit
                    } else {
                        updates::Phase::Installing
                    };
                    if !waiting {
                        this.persist(cx);
                    }
                    cx.notify();
                    waiting
                });
                match waiting {
                    Ok(false) => break,
                    Ok(true) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(50))
                            .await
                    }
                    Err(_) => return,
                }
            }
            let flush = owner
                .read_with(cx, |this, _| {
                    this.state_writer
                        .as_ref()
                        .map(crate::state::Writer::flush_handle)
                })
                .ok()
                .flatten();
            let saved = cx
                .background_spawn(async move { flush.map_or(Ok(()), crate::state::Flush::wait) })
                .await;
            if let Err(error) = saved {
                let _ = owner.update(cx, |this, cx| {
                    this.state_error = Some(error.clone());
                    this.updates.phase = updates::Phase::Error(error);
                    cx.notify();
                });
                return;
            }
            let result = cx
                .background_spawn(async move { release.package.install(bytes) })
                .await;
            let _ = owner.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.persist(cx);
                    cx.restart();
                }
                Err(error) => {
                    this.updates.phase = updates::Phase::Error(error);
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }
    pub(super) fn render_update_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use updates::Phase;
        let (message, button) = match &self.updates.phase {
            Phase::Idle => return None,
            Phase::Checking if !self.updates.show_check => return None,
            Phase::Checking => ("Checking for updates…".into(), None),
            Phase::Current => (
                "You're up to date. No newer published release is available.".into(),
                None,
            ),
            Phase::Available(version) => (
                format!("Diffy {version} is available."),
                Some("Update and Restart"),
            ),
            Phase::Downloading { received, total } => (
                match total {
                    Some(total) if *total > 0 => format!(
                        "Downloading update… {}%",
                        (received.saturating_mul(100) / total).min(100)
                    ),
                    _ => format!("Downloading update… {} KB", received / 1024),
                },
                None,
            ),
            Phase::WaitingForGit => ("Waiting for Git before restarting…".into(), None),
            Phase::Installing => ("Installing update…".into(), None),
            Phase::Error(error) => (
                error.clone(),
                Some(if self.updates.release.is_some() {
                    "Retry Update"
                } else {
                    "Retry Check"
                }),
            ),
        };
        Some(
            h_flex()
                .px_3()
                .py_2()
                .gap_3()
                .bg(rgb(0))
                .text_size(px(12.))
                .child(div().flex_1().min_w_0().child(message))
                .children(button.map(|label| {
                    Button::new("update-action")
                        .label(label)
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.updates.release.is_some() {
                                this.install_update(cx);
                            } else {
                                this.check_updates(true, cx);
                            }
                        }))
                }))
                .when(!self.updates.busy(), |row| {
                    row.child(
                        Button::new("dismiss-update")
                            .label("Dismiss")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.updates.dismiss();
                                cx.notify();
                            })),
                    )
                })
                .into_any_element(),
        )
    }
    #[cfg(feature = "ui-tests")]
    pub fn set_update_backend(&mut self, backend: Arc<dyn updates::Backend>) {
        self.updates.backend = backend;
    }
    #[cfg(feature = "ui-tests")]
    pub fn update_phase(&self) -> &updates::Phase {
        &self.updates.phase
    }
}
