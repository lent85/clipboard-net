use super::{
    detect_session_type, format_gnome_copied_files, format_uri_list, is_sensitive_mime_target,
    normalize_image_to_png, parse_gnome_copied_files, parse_uri_list, read_png_dimensions,
    ClipboardCommand, ClipboardEvent, ClipboardHealth, ClipboardPayload, HealthLevel,
};
use chrono::Utc;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{self, SelectionEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as XProtoExt,
    CreateWindowAux, EventMask, PropMode, Property, SelectionNotifyEvent, SelectionRequestEvent,
    Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperExt;

const INCR_THRESHOLD_BYTES: usize = 200 * 1024;
const INCR_CHUNK_BYTES: usize = 128 * 1024;

struct Atoms {
    clipboard: Atom,
    targets: Atom,
    timestamp: Atom,
    multiple: Atom,
    utf8_string: Atom,
    string: Atom,
    text: Atom,
    text_plain: Atom,
    text_plain_utf8: Atom,
    text_html: Atom,
    image_png: Atom,
    image_bmp: Atom,
    image_jpeg: Atom,
    uri_list: Atom,
    gnome_copied_files: Atom,
    incr: Atom,
    atom_type: Atom,
    integer_type: Atom,
    prop_read: Atom,
    wake_msg: Atom,
}

impl Atoms {
    fn intern_all(conn: &RustConnection) -> Result<Self, String> {
        let names = [
            "CLIPBOARD",
            "TARGETS",
            "TIMESTAMP",
            "MULTIPLE",
            "UTF8_STRING",
            "STRING",
            "TEXT",
            "text/plain",
            "text/plain;charset=utf-8",
            "text/html",
            "image/png",
            "image/bmp",
            "image/jpeg",
            "text/uri-list",
            "x-special/gnome-copied-files",
            "INCR",
            "ATOM",
            "INTEGER",
            "CLIPNET_READ_PROP",
            "CLIPNET_WAKE_MSG",
        ];
        let mut cookies = Vec::with_capacity(names.len());
        for name in names {
            cookies.push(
                conn.intern_atom(false, name.as_bytes())
                    .map_err(|e| e.to_string())?,
            );
        }
        let mut atoms = Vec::with_capacity(names.len());
        for c in cookies {
            atoms.push(c.reply().map_err(|e| e.to_string())?.atom);
        }
        Ok(Self {
            clipboard: atoms[0],
            targets: atoms[1],
            timestamp: atoms[2],
            multiple: atoms[3],
            utf8_string: atoms[4],
            string: atoms[5],
            text: atoms[6],
            text_plain: atoms[7],
            text_plain_utf8: atoms[8],
            text_html: atoms[9],
            image_png: atoms[10],
            image_bmp: atoms[11],
            image_jpeg: atoms[12],
            uri_list: atoms[13],
            gnome_copied_files: atoms[14],
            incr: atoms[15],
            atom_type: atoms[16],
            integer_type: atoms[17],
            prop_read: atoms[18],
            wake_msg: atoms[19],
        })
    }
}

struct ServedClipboard {
    targets: Vec<Atom>,
    entries: HashMap<Atom, Vec<u8>>,
    timestamp: u32,
}

impl ServedClipboard {
    fn from_payload(payload: &ClipboardPayload, atoms: &Atoms, timestamp: u32) -> Self {
        let mut entries = HashMap::new();
        let mut targets = vec![atoms.targets, atoms.timestamp];

        match payload {
            ClipboardPayload::Text(text) => {
                let bytes = text.as_bytes().to_vec();
                for a in [
                    atoms.utf8_string,
                    atoms.text_plain_utf8,
                    atoms.text_plain,
                    atoms.text,
                    atoms.string,
                ] {
                    targets.push(a);
                    entries.insert(a, bytes.clone());
                }
            }
            ClipboardPayload::Html { html, plain_text } => {
                targets.push(atoms.text_html);
                entries.insert(atoms.text_html, html.as_bytes().to_vec());

                let plain_bytes = plain_text.as_bytes().to_vec();
                for a in [
                    atoms.utf8_string,
                    atoms.text_plain_utf8,
                    atoms.text_plain,
                    atoms.string,
                ] {
                    targets.push(a);
                    entries.insert(a, plain_bytes.clone());
                }
            }
            ClipboardPayload::Image { png_bytes, .. } => {
                targets.push(atoms.image_png);
                entries.insert(atoms.image_png, png_bytes.clone());
            }
            ClipboardPayload::Files(paths) => {
                let gnome = format_gnome_copied_files(paths).into_bytes();
                let uri_list = format_uri_list(paths).into_bytes();
                let plain = paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes();
                targets.push(atoms.gnome_copied_files);
                entries.insert(atoms.gnome_copied_files, gnome);
                targets.push(atoms.uri_list);
                entries.insert(atoms.uri_list, uri_list);
                for a in [atoms.utf8_string, atoms.text_plain_utf8, atoms.text_plain] {
                    targets.push(a);
                    entries.insert(a, plain.clone());
                }
            }
        }

        Self {
            targets,
            entries,
            timestamp,
        }
    }
}

struct OutgoingIncr {
    data: Vec<u8>,
    offset: usize,
    target_type: Atom,
}

pub fn spawn_linux_clipboard(
    mut cmd_rx: mpsc::UnboundedReceiver<ClipboardCommand>,
    evt_tx: mpsc::UnboundedSender<ClipboardEvent>,
    skip_password_managers: Arc<AtomicBool>,
) {
    let session = detect_session_type();
    if std::env::var("DISPLAY")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        let (std_cmd_tx, std_cmd_rx) = std::sync::mpsc::channel::<ClipboardCommand>();
        let wake_window_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<(Window, Atom)>));
        let wake_slot_clone = wake_window_slot.clone();

        // Bridge tokio mpsc -> std mpsc + X11 ClientMessage wake
        tokio::spawn(async move {
            let wake_conn = RustConnection::connect(None).ok().map(|(c, _)| c);
            while let Some(cmd) = cmd_rx.recv().await {
                let is_shutdown = matches!(cmd, ClipboardCommand::Shutdown);
                let _ = std_cmd_tx.send(cmd);
                if let (Some(wc), Some((win, wake_atom))) =
                    (&wake_conn, *wake_slot_clone.lock().unwrap())
                {
                    let ev = ClientMessageEvent {
                        response_type: x11rb::protocol::xproto::CLIENT_MESSAGE_EVENT,
                        format: 32,
                        sequence: 0,
                        window: win,
                        type_: wake_atom,
                        data: ClientMessageData::from([1u32, 0, 0, 0, 0]),
                    };
                    let _ = wc.send_event(false, win, EventMask::NO_EVENT, ev);
                    let _ = wc.flush();
                }
                if is_shutdown {
                    break;
                }
            }
        });

        let evt_tx_clone = evt_tx.clone();
        let skip_clone = skip_password_managers.clone();
        std::thread::Builder::new()
            .name("clipnet-x11-clipboard".to_string())
            .spawn(move || {
                run_x11_loop(
                    std_cmd_rx,
                    evt_tx_clone,
                    wake_window_slot,
                    session,
                    skip_clone,
                );
            })
            .ok();
    } else if has_wl_clipboard() {
        let _ = skip_password_managers;
        tokio::spawn(async move {
            run_wl_clipboard_fallback(cmd_rx, evt_tx, session).await;
        });
    } else {
        let _ = evt_tx.send(ClipboardEvent::HealthChanged(ClipboardHealth {
            status: HealthLevel::Degraded,
            backend_name: "Unavailable".to_string(),
            session_type: session,
            detail: "Neither DISPLAY (X11/XWayland) nor wl-clipboard was detected. Copy/paste requires DISPLAY or wl-copy/wl-paste.".to_string(),
            last_capture_ms: None,
            skipped_privacy_count: 0,
        }));
    }
}

fn has_wl_clipboard() -> bool {
    Command::new("wl-paste")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_x11_loop(
    cmd_rx: std::sync::mpsc::Receiver<ClipboardCommand>,
    evt_tx: mpsc::UnboundedSender<ClipboardEvent>,
    wake_slot: std::sync::Arc<std::sync::Mutex<Option<(Window, Atom)>>>,
    session: String,
    skip_password_managers: Arc<AtomicBool>,
) {
    let backend_name = if std::env::var("WAYLAND_DISPLAY")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        "XWayland (XFixes)".to_string()
    } else {
        "X11 (XFixes)".to_string()
    };

    let (conn, screen_num) = match RustConnection::connect(None) {
        Ok(c) => c,
        Err(e) => {
            let _ = evt_tx.send(ClipboardEvent::HealthChanged(ClipboardHealth {
                status: HealthLevel::Error,
                backend_name,
                session_type: session,
                detail: format!("Failed to connect to X11/XWayland display: {e}"),
                last_capture_ms: None,
                skipped_privacy_count: 0,
            }));
            return;
        }
    };

    let atoms = match Atoms::intern_all(&conn) {
        Ok(a) => a,
        Err(e) => {
            let _ = evt_tx.send(ClipboardEvent::HealthChanged(ClipboardHealth {
                status: HealthLevel::Error,
                backend_name,
                session_type: session,
                detail: format!("Failed to intern X11 atoms: {e}"),
                last_capture_ms: None,
                skipped_privacy_count: 0,
            }));
            return;
        }
    };

    if let Err(e) = xfixes::query_version(&conn, 5, 0)
        .map_err(|e| e.to_string())
        .and_then(|c| c.reply().map_err(|e| e.to_string()))
    {
        let _ = evt_tx.send(ClipboardEvent::HealthChanged(ClipboardHealth {
            status: HealthLevel::Error,
            backend_name,
            session_type: session,
            detail: format!("XFixes extension unavailable: {e}"),
            last_capture_ms: None,
            skipped_privacy_count: 0,
        }));
        return;
    }

    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let our_win = match conn.generate_id() {
        Ok(id) => id,
        Err(e) => {
            warn!("failed to generate X11 window id: {e}");
            return;
        }
    };

    if let Err(e) = conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        our_win,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    ) {
        warn!("failed to create hidden X11 clipboard window: {e}");
        return;
    }

    if let Err(e) = xfixes::select_selection_input(
        &conn,
        root,
        atoms.clipboard,
        SelectionEventMask::SET_SELECTION_OWNER,
    ) {
        let _ = evt_tx.send(ClipboardEvent::HealthChanged(ClipboardHealth {
            status: HealthLevel::Error,
            backend_name,
            session_type: session,
            detail: format!("Failed to subscribe to XFixes clipboard events: {e}"),
            last_capture_ms: None,
            skipped_privacy_count: 0,
        }));
        return;
    }
    let _ = conn.flush();

    *wake_slot.lock().unwrap() = Some((our_win, atoms.wake_msg));

    let mut health = ClipboardHealth {
        status: HealthLevel::Healthy,
        backend_name,
        session_type: session,
        detail: "Event-driven XFixes clipboard monitor active".to_string(),
        last_capture_ms: None,
        skipped_privacy_count: 0,
    };
    let _ = evt_tx.send(ClipboardEvent::HealthChanged(health.clone()));

    let mut served: Option<ServedClipboard> = None;
    let mut outgoing_incr: HashMap<(Window, Atom), OutgoingIncr> = HashMap::new();
    let mut atom_name_cache: HashMap<Atom, String> = HashMap::new();
    let mut last_event_capture = Instant::now() - Duration::from_secs(10);

    loop {
        // Drain pending commands before waiting for X11 event
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                ClipboardCommand::Shutdown => return,
                ClipboardCommand::SetClipboard(payload) => {
                    let ts = Utc::now().timestamp_millis() as u32;
                    served = Some(ServedClipboard::from_payload(&payload, &atoms, ts));
                    if let Err(e) =
                        conn.set_selection_owner(our_win, atoms.clipboard, x11rb::CURRENT_TIME)
                    {
                        warn!("failed to set X11 clipboard owner: {e}");
                    }
                    let _ = conn.flush();
                }
            }
        }

        let ev = match conn.wait_for_event() {
            Ok(ev) => ev,
            Err(e) => {
                health.status = HealthLevel::Error;
                health.detail = format!("X11 connection closed: {e}");
                let _ = evt_tx.send(ClipboardEvent::HealthChanged(health.clone()));
                return;
            }
        };

        match ev {
            Event::ClientMessage(_) => {
                // Wake signal to drain cmd_rx on next loop turn
                continue;
            }
            Event::XfixesSelectionNotify(sel) => {
                if sel.owner == our_win || sel.owner == x11rb::NONE {
                    continue;
                }
                if last_event_capture.elapsed() < Duration::from_millis(80) {
                    std::thread::sleep(Duration::from_millis(80));
                }
                last_event_capture = Instant::now();

                match read_x11_clipboard(
                    &conn,
                    our_win,
                    &atoms,
                    &mut atom_name_cache,
                    skip_password_managers.load(Ordering::Relaxed),
                ) {
                    ReadOutcome::Payload(payload) => {
                        let byte_len = payload.byte_len();
                        let content_hash = payload.content_hash();
                        health.last_capture_ms = Some(Utc::now().timestamp_millis());
                        health.status = HealthLevel::Healthy;
                        let _ = evt_tx.send(ClipboardEvent::HealthChanged(health.clone()));
                        let _ = evt_tx.send(ClipboardEvent::Captured {
                            payload,
                            content_hash,
                            byte_len,
                        });
                    }
                    ReadOutcome::SkippedPrivacy(reason) => {
                        health.skipped_privacy_count += 1;
                        let _ = evt_tx.send(ClipboardEvent::HealthChanged(health.clone()));
                        let _ = evt_tx.send(ClipboardEvent::SkippedPrivacy { reason });
                    }
                    ReadOutcome::Empty => {}
                }
            }
            Event::SelectionClear(clr) => {
                if clr.selection == atoms.clipboard {
                    served = None;
                }
            }
            Event::SelectionRequest(req) => {
                handle_selection_request(&conn, our_win, &atoms, &req, &served, &mut outgoing_incr);
            }
            Event::PropertyNotify(prop) if prop.state == Property::DELETE => {
                handle_incr_delete(&conn, &prop, &mut outgoing_incr);
            }
            _ => {}
        }
    }
}

enum ReadOutcome {
    Payload(ClipboardPayload),
    SkippedPrivacy(String),
    Empty,
}

fn get_atom_name(
    conn: &RustConnection,
    atom: Atom,
    cache: &mut HashMap<Atom, String>,
) -> Option<String> {
    if let Some(name) = cache.get(&atom) {
        return Some(name.clone());
    }
    let reply = conn.get_atom_name(atom).ok()?.reply().ok()?;
    let name = String::from_utf8_lossy(&reply.name).into_owned();
    cache.insert(atom, name.clone());
    Some(name)
}

fn read_x11_clipboard(
    conn: &RustConnection,
    win: Window,
    atoms: &Atoms,
    atom_cache: &mut HashMap<Atom, String>,
    skip_password_managers: bool,
) -> ReadOutcome {
    let Some(targets_bytes) = request_selection_target(conn, win, atoms, atoms.targets) else {
        return ReadOutcome::Empty;
    };

    let target_atoms: Vec<Atom> = targets_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&c| u32::from_ne_bytes(c))
        .collect();

    let has_files_or_image = target_atoms.contains(&atoms.gnome_copied_files)
        || target_atoms.contains(&atoms.uri_list)
        || target_atoms.contains(&atoms.image_png)
        || target_atoms.contains(&atoms.image_bmp)
        || target_atoms.contains(&atoms.image_jpeg);

    if skip_password_managers && !has_files_or_image {
        let mut target_names = Vec::with_capacity(target_atoms.len());
        for &a in &target_atoms {
            if let Some(n) = get_atom_name(conn, a, atom_cache) {
                target_names.push(n);
            }
        }

        if let Some(reason_target) = is_sensitive_mime_target(&target_names) {
            debug!("Skipping sensitive clipboard target: {reason_target}");
            return ReadOutcome::SkippedPrivacy(format!(
                "Sensitive MIME target detected: {reason_target}"
            ));
        }
    }

    // 1. Files
    if target_atoms.contains(&atoms.gnome_copied_files) {
        if let Some(data) = request_selection_target(conn, win, atoms, atoms.gnome_copied_files) {
            let paths = parse_gnome_copied_files(&String::from_utf8_lossy(&data));
            if !paths.is_empty() {
                return ReadOutcome::Payload(ClipboardPayload::Files(paths));
            }
        }
    }
    if target_atoms.contains(&atoms.uri_list) {
        if let Some(data) = request_selection_target(conn, win, atoms, atoms.uri_list) {
            let paths = parse_uri_list(&String::from_utf8_lossy(&data));
            if !paths.is_empty() {
                return ReadOutcome::Payload(ClipboardPayload::Files(paths));
            }
        }
    }

    // 2. Images
    for img_atom in [atoms.image_png, atoms.image_bmp, atoms.image_jpeg] {
        if target_atoms.contains(&img_atom) {
            if let Some(bytes) = request_selection_target(conn, win, atoms, img_atom) {
                if img_atom == atoms.image_png {
                    if let Some((w, h)) = read_png_dimensions(&bytes) {
                        return ReadOutcome::Payload(ClipboardPayload::Image {
                            png_bytes: bytes,
                            width: w,
                            height: h,
                        });
                    }
                }
                if let Some((png_bytes, w, h)) = normalize_image_to_png(&bytes) {
                    return ReadOutcome::Payload(ClipboardPayload::Image {
                        png_bytes,
                        width: w,
                        height: h,
                    });
                }
            }
        }
    }

    // 3. HTML (+ plain text)
    if target_atoms.contains(&atoms.text_html) {
        if let Some(html_bytes) = request_selection_target(conn, win, atoms, atoms.text_html) {
            let html = decode_x11_text(&html_bytes);
            if !html.trim().is_empty() {
                let plain_text = read_plain_text_from_targets(conn, win, atoms, &target_atoms)
                    .unwrap_or_default();
                return ReadOutcome::Payload(ClipboardPayload::Html { html, plain_text });
            }
        }
    }

    // 4. Plain Text
    if let Some(text) = read_plain_text_from_targets(conn, win, atoms, &target_atoms) {
        if !text.is_empty() {
            return ReadOutcome::Payload(ClipboardPayload::Text(text));
        }
    }

    ReadOutcome::Empty
}

fn read_plain_text_from_targets(
    conn: &RustConnection,
    win: Window,
    atoms: &Atoms,
    target_atoms: &[Atom],
) -> Option<String> {
    for &cand in &[
        atoms.utf8_string,
        atoms.text_plain_utf8,
        atoms.text_plain,
        atoms.text,
        atoms.string,
    ] {
        if target_atoms.contains(&cand) {
            if let Some(bytes) = request_selection_target(conn, win, atoms, cand) {
                let s = decode_x11_text(&bytes);
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn decode_x11_text(bytes: &[u8]) -> String {
    // Handle UTF-16 BOM often emitted by Chromium/Wine text/html on X11
    if bytes.starts_with(&[0xFF, 0xFE]) && bytes.len() >= 2 {
        let u16s: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_string()
}

fn request_selection_target(
    conn: &RustConnection,
    win: Window,
    atoms: &Atoms,
    target: Atom,
) -> Option<Vec<u8>> {
    let _ = conn.delete_property(win, atoms.prop_read);
    conn.convert_selection(
        win,
        atoms.clipboard,
        target,
        atoms.prop_read,
        x11rb::CURRENT_TIME,
    )
    .ok()?;
    let _ = conn.flush();

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if Instant::now() > deadline {
            return None;
        }
        match conn.poll_for_event().ok()? {
            Some(Event::SelectionNotify(notify)) => {
                if notify.selection != atoms.clipboard || notify.target != target {
                    continue;
                }
                if notify.property == x11rb::NONE {
                    return None;
                }
                let reply = conn
                    .get_property(true, win, notify.property, AtomEnum::ANY, 0, u32::MAX / 4)
                    .ok()?
                    .reply()
                    .ok()?;

                if reply.type_ == atoms.incr {
                    return read_incr_property(conn, win, notify.property);
                }
                return Some(reply.value);
            }
            Some(_) => continue,
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

fn read_incr_property(conn: &RustConnection, win: Window, prop: Atom) -> Option<Vec<u8>> {
    let mut accumulated = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if Instant::now() > deadline {
            return None;
        }
        match conn.poll_for_event().ok()? {
            Some(Event::PropertyNotify(pn))
                if pn.window == win && pn.atom == prop && pn.state == Property::NEW_VALUE =>
            {
                let reply = conn
                    .get_property(true, win, prop, AtomEnum::ANY, 0, u32::MAX / 4)
                    .ok()?
                    .reply()
                    .ok()?;
                if reply.value.is_empty() {
                    return Some(accumulated);
                }
                accumulated.extend_from_slice(&reply.value);
            }
            Some(_) => continue,
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

fn handle_selection_request(
    conn: &RustConnection,
    our_win: Window,
    atoms: &Atoms,
    req: &SelectionRequestEvent,
    served: &Option<ServedClipboard>,
    outgoing_incr: &mut HashMap<(Window, Atom), OutgoingIncr>,
) {
    let mut notify = SelectionNotifyEvent {
        response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: req.time,
        requestor: req.requestor,
        selection: req.selection,
        target: req.target,
        property: x11rb::NONE,
    };

    if req.selection != atoms.clipboard {
        let _ = conn.send_event(false, req.requestor, EventMask::NO_EVENT, notify);
        let _ = conn.flush();
        return;
    }

    let Some(data) = served else {
        let _ = conn.send_event(false, req.requestor, EventMask::NO_EVENT, notify);
        let _ = conn.flush();
        return;
    };

    let prop = if req.property == x11rb::NONE {
        req.target
    } else {
        req.property
    };

    let write_ok = if req.target == atoms.targets {
        let _ = conn.change_property32(
            PropMode::REPLACE,
            req.requestor,
            prop,
            atoms.atom_type,
            &data.targets,
        );
        true
    } else if req.target == atoms.timestamp {
        let _ = conn.change_property32(
            PropMode::REPLACE,
            req.requestor,
            prop,
            atoms.integer_type,
            &[data.timestamp],
        );
        true
    } else if req.target == atoms.multiple {
        // Basic MULTIPLE support: read pairs of (target, property)
        if let Ok(Ok(rep)) = conn
            .get_property(false, req.requestor, prop, AtomEnum::ANY, 0, 1024)
            .map(|c| c.reply())
        {
            let pair_atoms: Vec<Atom> = rep
                .value
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&c| u32::from_ne_bytes(c))
                .collect();
            for &[t, p] in pair_atoms.as_chunks::<2>().0 {
                if let Some(payload) = data.entries.get(&t) {
                    let _ = conn.change_property8(PropMode::REPLACE, req.requestor, p, t, payload);
                }
            }
            true
        } else {
            false
        }
    } else if let Some(payload) = data.entries.get(&req.target) {
        if payload.len() > INCR_THRESHOLD_BYTES {
            let _ = conn.change_window_attributes(
                req.requestor,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE),
            );
            let _ = conn.change_property32(
                PropMode::REPLACE,
                req.requestor,
                prop,
                atoms.incr,
                &[payload.len() as u32],
            );
            outgoing_incr.insert(
                (req.requestor, prop),
                OutgoingIncr {
                    data: payload.clone(),
                    offset: 0,
                    target_type: req.target,
                },
            );
        } else {
            let _ =
                conn.change_property8(PropMode::REPLACE, req.requestor, prop, req.target, payload);
        }
        true
    } else {
        false
    };

    if write_ok {
        notify.property = prop;
    }
    let _ = our_win;
    let _ = conn.send_event(false, req.requestor, EventMask::NO_EVENT, notify);
    let _ = conn.flush();
}

fn handle_incr_delete(
    conn: &RustConnection,
    prop: &x11rb::protocol::xproto::PropertyNotifyEvent,
    outgoing_incr: &mut HashMap<(Window, Atom), OutgoingIncr>,
) {
    let key = (prop.window, prop.atom);
    let Some(state) = outgoing_incr.get_mut(&key) else {
        return;
    };

    if state.offset >= state.data.len() {
        let _ = conn.change_property8(
            PropMode::REPLACE,
            prop.window,
            prop.atom,
            state.target_type,
            &[],
        );
        let _ = conn.flush();
        outgoing_incr.remove(&key);
        return;
    }

    let end = (state.offset + INCR_CHUNK_BYTES).min(state.data.len());
    let chunk = &state.data[state.offset..end];
    state.offset = end;
    let _ = conn.change_property8(
        PropMode::REPLACE,
        prop.window,
        prop.atom,
        state.target_type,
        chunk,
    );
    let _ = conn.flush();
}

async fn run_wl_clipboard_fallback(
    mut cmd_rx: mpsc::UnboundedReceiver<ClipboardCommand>,
    evt_tx: mpsc::UnboundedSender<ClipboardEvent>,
    session: String,
) {
    let mut health = ClipboardHealth {
        status: HealthLevel::Degraded,
        backend_name: "wl-clipboard CLI".to_string(),
        session_type: session,
        detail: "Running via wl-paste/wl-copy fallback (text & PNG)".to_string(),
        last_capture_ms: None,
        skipped_privacy_count: 0,
    };
    let _ = evt_tx.send(ClipboardEvent::HealthChanged(health.clone()));

    let mut last_hash = String::new();
    let mut interval = tokio::time::interval(Duration::from_millis(650));

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(ClipboardCommand::Shutdown) | None => break,
                    Some(ClipboardCommand::SetClipboard(payload)) => {
                        match payload {
                            ClipboardPayload::Text(t) => {
                                let _ = spawn_wl_copy(&["--type", "text/plain;charset=utf-8"], t.as_bytes());
                            }
                            ClipboardPayload::Html { html, .. } => {
                                let _ = spawn_wl_copy(&["--type", "text/html"], html.as_bytes());
                            }
                            ClipboardPayload::Image { png_bytes, .. } => {
                                let _ = spawn_wl_copy(&["--type", "image/png"], &png_bytes);
                            }
                            ClipboardPayload::Files(paths) => {
                                let uri = format_uri_list(&paths);
                                let _ = spawn_wl_copy(&["--type", "text/uri-list"], uri.as_bytes());
                            }
                        }
                    }
                }
            }
            _ = interval.tick() => {
                if let Ok(out) = Command::new("wl-paste").args(["--no-newline", "--type", "text/plain"]).output() {
                    if out.status.success() && !out.stdout.is_empty() {
                        let text = String::from_utf8_lossy(&out.stdout).into_owned();
                        let payload = ClipboardPayload::Text(text);
                        let hash = payload.content_hash();
                        if hash != last_hash {
                            last_hash = hash.clone();
                            let byte_len = payload.byte_len();
                            health.last_capture_ms = Some(Utc::now().timestamp_millis());
                            let _ = evt_tx.send(ClipboardEvent::HealthChanged(health.clone()));
                            let _ = evt_tx.send(ClipboardEvent::Captured {
                                payload,
                                content_hash: hash,
                                byte_len,
                            });
                        }
                    }
                }
            }
        }
    }
    info!("wl-clipboard fallback exited");
}

fn spawn_wl_copy(args: &[&str], data: &[u8]) -> std::io::Result<()> {
    let mut child = Command::new("wl-copy")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(data);
    }
    Ok(())
}
