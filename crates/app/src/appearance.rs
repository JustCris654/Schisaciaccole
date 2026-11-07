//! Observes the desktop's reduced-motion preference without blocking the UI.
//!
//! Linux uses the XDG Settings portal; macOS uses NSWorkspace accessibility
//! display preferences. Missing, malformed, or unsupported settings mean no
//! preference (`false`). Other platforms use the same fallback. A disconnected
//! portal falls back to false; observation resumes when the app is restarted.

/// Keeps native reduced-motion observation active until dropped.
pub struct ReducedMotionObserver {
    _observer: platform::Observer,
}

/// Delivers the initial reduced-motion preference and subsequent native changes.
///
/// Call this on the main thread and retain the returned guard while the UI runs.
/// The callback must marshal UI work to its event loop: Linux invokes it from a
/// background thread. Linux's initial result is asynchronous. Unavailable native
/// settings, unsupported platforms, or initialization failures deliver `false`.
/// Dropping the guard cancels observation without waiting for the worker thread.
pub fn observe_reduced_motion(
    on_change: impl Fn(bool) + Send + Sync + 'static,
) -> ReducedMotionObserver {
    ReducedMotionObserver {
        _observer: platform::observe(on_change),
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{marker::PhantomData, ptr::NonNull, sync::Arc};

    use objc2::{MainThreadMarker, rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
    use objc2_foundation::{
        NSNotification, NSNotificationCenter, NSObjectProtocol, NSOperationQueue,
    };

    pub(super) struct Observer {
        registration: Option<Registration>,
        _main_thread: PhantomData<*mut ()>,
    }

    struct Registration {
        center: Retained<NSNotificationCenter>,
        token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    }

    impl Drop for Observer {
        /// Removes the observer on its registering main thread; empty guards do nothing.
        fn drop(&mut self) {
            if let Some(registration) = &self.registration {
                // SAFETY: This is the exact token returned by this center; the guard
                // cannot move to another thread and keeps both objects alive.
                unsafe {
                    registration
                        .center
                        .removeObserver(registration.token.as_ref())
                };
            }
        }
    }

    /// Registers workspace notifications on the main queue and reads the local value.
    /// Calls from another thread fall back to false without touching AppKit.
    pub(super) fn observe(on_change: impl Fn(bool) + Send + Sync + 'static) -> Observer {
        let mut observer = Observer {
            registration: None,
            _main_thread: PhantomData,
        };
        if MainThreadMarker::new().is_none() {
            log::debug!("Reduced-motion observation requires the macOS main thread");
            on_change(false);
            return observer;
        }
        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let on_change = Arc::new(on_change);
        let block_callback = Arc::clone(&on_change);
        let block = block2::RcBlock::new(move |_: NonNull<NSNotification>| {
            block_callback(NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion());
        });
        // SAFETY: The native notification name is valid, no object filter is
        // required, and the Send + Sync callback runs on the main operation queue.
        // NSNotificationCenter copies the block and the guard owns its token.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        observer.registration = Some(Registration { center, token });
        on_change(workspace.accessibilityDisplayShouldReduceMotion());
        observer
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    pub(super) struct Observer;

    /// Supplies the no-preference fallback on platforms without native integration.
    pub(super) fn observe(on_change: impl Fn(bool) + Send + Sync + 'static) -> Observer {
        on_change(false);
        Observer
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{sync::Arc, time::Duration};

    use anyhow::Context;
    use futures_lite::StreamExt;
    use zbus::zvariant::{OwnedValue, Value};

    const APPEARANCE_NAMESPACE: &str = "org.freedesktop.appearance";
    const REDUCED_MOTION_KEY: &str = "reduced-motion";

    pub(super) struct Observer {
        _cancel: async_channel::Sender<()>,
    }

    /// Starts a cancellable portal worker; thread-creation failure delivers false.
    pub(super) fn observe(on_change: impl Fn(bool) + Send + Sync + 'static) -> Observer {
        let (cancel, cancelled) = async_channel::bounded::<()>(1);
        let on_change = Arc::new(on_change);
        let worker_callback = Arc::clone(&on_change);
        let worker = std::thread::Builder::new()
            .name("appearance-preferences".into())
            .spawn(move || {
                async_io::block_on(futures_lite::future::race(
                    async {
                        if let Err(error) = observe_portal(worker_callback.as_ref()).await {
                            log::debug!("Reduced-motion portal unavailable: {error:#}");
                            worker_callback(false);
                        }
                    },
                    async {
                        let _ = cancelled.recv().await;
                    },
                ));
            });
        if let Err(error) = worker {
            log::debug!("Could not start reduced-motion observer: {error}");
            on_change(false);
        }
        Observer { _cancel: cancel }
    }

    /// Reads the initial preference, then consumes matching portal signals.
    ///
    /// A missing setting falls back to false while continuing to observe changes.
    ///
    /// # Errors
    /// Returns an error if the session bus, proxy, or signal stream is unavailable.
    async fn observe_portal(on_change: &(impl Fn(bool) + Send + Sync)) -> anyhow::Result<()> {
        let connection = zbus::connection::Builder::session()?
            .method_timeout(Duration::from_secs(2))
            .build()
            .await
            .context("connect to the session bus")?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        )
        .await
        .context("create the Settings portal proxy")?;
        let mut changes = proxy
            .receive_signal("SettingChanged")
            .await
            .context("subscribe to appearance changes")?;
        let initial = match read_preference(&proxy).await {
            Ok(preference) => preference,
            Err(error) => {
                log::debug!("Could not read reduced-motion preference: {error}");
                false
            }
        };
        on_change(initial);
        while let Some(message) = changes.next().await {
            match changed_preference(&message) {
                Ok(Some(preference)) => on_change(preference),
                Ok(None) => {}
                Err(error) => log::debug!("Invalid Settings portal signal: {error}"),
            }
        }
        anyhow::bail!("Settings portal signal stream closed")
    }

    /// Reads either portal API version, falling back to no preference for bad data.
    ///
    /// # Errors
    /// Returns the legacy Read error if both ReadOne and Read fail or time out.
    async fn read_preference(proxy: &zbus::Proxy<'_>) -> zbus::Result<bool> {
        let arguments = &(APPEARANCE_NAMESPACE, REDUCED_MOTION_KEY);
        let value: OwnedValue = match proxy.call("ReadOne", arguments).await {
            Ok(value) => value,
            Err(_) => proxy.call("Read", arguments).await?,
        };
        Ok(preference_from_value(&value))
    }

    /// Decodes both ReadOne's value and the legacy Read's extra variant layer.
    /// Unknown numbers and malformed types mean no preference, per the portal API.
    fn preference_from_value(mut value: &Value<'_>) -> bool {
        while let Value::Value(inner) = value {
            value = inner;
        }
        matches!(value, Value::U32(1))
    }

    /// Returns only appearance reduced-motion changes, ignoring other settings.
    /// Unknown preference values mean false.
    ///
    /// # Errors
    /// Returns an error if the signal body does not match the portal signature.
    fn changed_preference(message: &zbus::Message) -> zbus::Result<Option<bool>> {
        let body = message.body();
        let (namespace, key, value): (&str, &str, OwnedValue) = body.deserialize()?;
        if namespace != APPEARANCE_NAMESPACE || key != REDUCED_MOTION_KEY {
            return Ok(None);
        }
        Ok(Some(preference_from_value(&value)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reads_reduced_motion_preference() {
            assert!(preference_from_value(&Value::U32(1)));
        }

        #[test]
        fn reads_legacy_nested_variant() {
            let value = Value::Value(Box::new(Value::U32(1)));
            assert!(preference_from_value(&value));
        }

        #[test]
        fn defaults_unknown_preferences_to_false() {
            assert!(!preference_from_value(&Value::U32(0)));
            assert!(!preference_from_value(&Value::U32(2)));
            assert!(!preference_from_value(&Value::U32(u32::MAX)));
        }

        #[test]
        fn defaults_malformed_types_to_false() {
            assert!(!preference_from_value(&Value::Bool(true)));
            assert!(!preference_from_value(&Value::new("1")));
        }

        #[test]
        fn decodes_matching_preference_change() {
            let message = zbus::Message::signal("/test", "org.test.Settings", "SettingChanged")
                .unwrap()
                .build(&(APPEARANCE_NAMESPACE, REDUCED_MOTION_KEY, Value::U32(1)))
                .unwrap();
            assert_eq!(changed_preference(&message).unwrap(), Some(true));
        }

        #[test]
        fn decodes_preference_being_disabled() {
            let message = zbus::Message::signal("/test", "org.test.Settings", "SettingChanged")
                .unwrap()
                .build(&(APPEARANCE_NAMESPACE, REDUCED_MOTION_KEY, Value::U32(0)))
                .unwrap();
            assert_eq!(changed_preference(&message).unwrap(), Some(false));
        }

        #[test]
        fn ignores_unrelated_preference_changes() {
            for (namespace, key) in [
                (APPEARANCE_NAMESPACE, "color-scheme"),
                ("org.test.appearance", REDUCED_MOTION_KEY),
            ] {
                let message = zbus::Message::signal("/test", "org.test.Settings", "SettingChanged")
                    .unwrap()
                    .build(&(namespace, key, Value::U32(1)))
                    .unwrap();
                assert_eq!(changed_preference(&message).unwrap(), None);
            }
        }

        #[test]
        fn rejects_malformed_signal_bodies() {
            let message = zbus::Message::signal("/test", "org.test.Settings", "SettingChanged")
                .unwrap()
                .build(&(APPEARANCE_NAMESPACE, REDUCED_MOTION_KEY))
                .unwrap();
            assert!(changed_preference(&message).is_err());
        }
    }
}
