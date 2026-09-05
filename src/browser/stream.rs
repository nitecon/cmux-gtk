//! Preview stream transport and GTK delivery with latest-frame ownership.
use super::{frames, metrics, pixels};
use futures_util::StreamExt;
use gtk4::prelude::*;

/// Start the stream reader and weak-widget delivery; the caller owns cancelling the returned task.
pub(super) fn start(
    runtime: &tokio::runtime::Handle,
    port: u16,
    picture: gtk4::Picture,
) -> tokio::task::JoinHandle<()> {
    let (frame_tx, frame_rx) = tokio::sync::watch::channel(None::<glib::Bytes>);
    glib::MainContext::default().spawn_local(deliver(picture, frame_rx, runtime.clone()));
    runtime.spawn(receive(format!("ws://127.0.0.1:{port}"), frame_tx))
}

/// Receive bounded envelopes until transport failure, task cancellation or delivery receiver closure.
async fn receive(url: String, frame_tx: tokio::sync::watch::Sender<Option<glib::Bytes>>) {
    let _stream_metrics = metrics::Stream::begin();
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
        max_message_size: Some(8 * 1024 * 1024),
        max_frame_size: Some(8 * 1024 * 1024),
        ..Default::default()
    };
    let ws_result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio_tungstenite::connect_async_with_config(&url, Some(config), false),
    )
    .await;
    let (ws_stream, _) = match ws_result {
        Ok(Ok(conn)) => conn,
        Ok(Err(_)) => {
            crate::diagnostics::record(
                "browser.stream.connect",
                serde_json::json!({"outcome": "error"}),
            );
            return;
        }
        Err(_) => {
            crate::diagnostics::record(
                "browser.stream.connect",
                serde_json::json!({"outcome": "timeout"}),
            );
            return;
        }
    };
    crate::diagnostics::record(
        "browser.stream.connect",
        serde_json::json!({"outcome": "success"}),
    );
    let (_write, mut read) = ws_stream.split();
    loop {
        let Some(msg_result) = (tokio::select! {
            _ = frame_tx.closed() => return,
            message = read.next() => message,
        }) else {
            break;
        };
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                eprintln!("cmux: browser stream error: {}", e);
                break;
            }
        };
        if let tokio_tungstenite::tungstenite::Message::Text(text) = &msg {
            match frames::decode(text) {
                Ok(Some(bytes)) => {
                    metrics::received(bytes.len());
                    if frame_tx.send(Some(bytes)).is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(_) => metrics::invalid_base64(),
            }
        }
    }
}

/// Assign the latest shared frame to a weakly held GTK picture and hide its initial empty overlay.
async fn deliver(
    picture: gtk4::Picture,
    mut frame_rx: tokio::sync::watch::Receiver<Option<glib::Bytes>>,
    runtime: tokio::runtime::Handle,
) {
    let (destroyed_tx, mut destroyed_rx) = tokio::sync::oneshot::channel();
    let destruction = picture.add_weak_ref_notify_local(move || {
        let _ = destroyed_tx.send(());
    });
    let picture_weak = picture.downgrade();
    drop(picture);

    let mut first_frame = true;
    loop {
        tokio::select! {
            _ = &mut destroyed_rx => break,
            changed = frame_rx.changed() => { if changed.is_err() { break; } }
        }
        let Some(bytes) = frame_rx.borrow_and_update().clone() else {
            continue;
        };
        let decoded = tokio::select! {
            _ = &mut destroyed_rx => break,
            pixels = pixels::decode(&runtime, bytes) => pixels,
        };
        let Some(pixels) = decoded else {
            continue;
        };
        let Some(picture_clone) = picture_weak.upgrade() else {
            break;
        };
        let texture = gtk4::gdk::MemoryTexture::new(
            pixels.width,
            pixels.height,
            gtk4::gdk::MemoryFormat::R8g8b8a8,
            &pixels.bytes,
            pixels.width as usize * 4,
        );
        picture_clone.set_paintable(Some(&texture));
        metrics::texture(true);
        // Hide the "No browser preview" overlay label on first frame
        if first_frame {
            first_frame = false;
            if let Some(overlay) = picture_clone
                .parent()
                .and_then(|p| p.downcast::<gtk4::Overlay>().ok())
            {
                if let Some(child) = overlay.first_child() {
                    let mut sibling = child.next_sibling();
                    while let Some(widget) = sibling {
                        let next = widget.next_sibling();
                        if widget.has_css_class("preview-empty") {
                            widget.set_visible(false);
                        }
                        sibling = next;
                    }
                }
            }
        }
    }
    destruction.disconnect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::SinkExt;

    /// Verify idle destruction, background decode and GTK texture assignment in one GTK thread.
    #[test]
    #[ignore = "requires GTK display; run in headless Linux CI"]
    fn destroyed_picture_releases_delivery() {
        gtk4::init().unwrap();
        let context = glib::MainContext::default();
        let (sender, receiver) = tokio::sync::watch::channel(None);
        let picture = gtk4::Picture::new();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let task =
            context.spawn_local(deliver(picture.clone(), receiver, runtime.handle().clone()));
        drop(picture);
        context.block_on(async {
            match futures_util::future::select(
                Box::pin(task),
                Box::pin(glib::timeout_future_seconds(5)),
            )
            .await
            {
                futures_util::future::Either::Left((result, _)) => result.unwrap(),
                futures_util::future::Either::Right(_) => {
                    panic!("destroyed picture retained idle delivery")
                }
            }
        });
        assert!(sender.is_closed());

        use image::ImageEncoder;
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 128], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let (sender, receiver) = tokio::sync::watch::channel(None);
        let picture = gtk4::Picture::new();
        let task =
            context.spawn_local(deliver(picture.clone(), receiver, runtime.handle().clone()));
        sender.send(Some(glib::Bytes::from_owned(png))).unwrap();
        context.block_on(async {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while picture.paintable().is_none() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "decoded frame was not assigned"
                );
                glib::timeout_future(std::time::Duration::from_millis(10)).await;
            }
            let paintable = picture.paintable().unwrap();
            assert_eq!(
                (paintable.intrinsic_width(), paintable.intrinsic_height()),
                (1, 1)
            );
            drop(picture);
            match futures_util::future::select(
                Box::pin(task),
                Box::pin(glib::timeout_future_seconds(5)),
            )
            .await
            {
                futures_util::future::Either::Left((result, _)) => result.unwrap(),
                futures_util::future::Either::Right(_) => {
                    panic!("displayed picture retained delivery")
                }
            }
        });
        assert!(sender.is_closed());
    }

    /// Deliver a real WebSocket envelope and retire an idle reader once its consumer disappears.
    #[tokio::test]
    async fn frame_delivery_and_receiver_cleanup() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (tx, mut rx) = tokio::sync::watch::channel(None);
        let reader = tokio::spawn(receive(url, tx));
        let (socket, _) = listener.accept().await.unwrap();
        let mut peer = tokio_tungstenite::accept_async(socket).await.unwrap();
        peer.send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"type":"frame","data":"AQ=="}"#.into(),
        ))
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rx.borrow_and_update().as_ref().unwrap().as_ref(), &[1]);
        drop(rx);
        tokio::time::timeout(std::time::Duration::from_secs(5), reader)
            .await
            .unwrap()
            .unwrap();
    }
}
