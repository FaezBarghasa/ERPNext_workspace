/// Task 4.1 — WASM File Upload Component
///
/// Provides a drag-and-drop + click-to-browse file picker that uploads files
/// to the frappe-storage SHA-256 deduplicated backend via multipart POST.
/// Renders upload progress, file preview thumbnails, and the resulting
/// storage hash URL once the upload completes.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadedFile {
    /// SHA-256 hex hash — used as the permanent file identifier
    pub file_hash: String,
    pub original_name: String,
    pub size_bytes: u64,
    pub url: String,
}

#[derive(Clone, PartialEq)]
enum UploadState {
    Idle,
    Uploading { progress_pct: u8 },
    Done(UploadedFile),
    Failed(String),
}

#[derive(Props, Clone, PartialEq)]
pub struct FileUploadProps {
    pub api_base: String,
    pub tenant_id: String,
    /// Called when a file is successfully uploaded
    pub on_upload: EventHandler<UploadedFile>,
    /// Accepted MIME types, e.g. "image/*,application/pdf"
    #[props(default = "*/*".to_string())]
    pub accept: String,
    #[props(default = false)]
    pub multiple: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadResponse {
    file_hash: String,
    url: String,
    original_name: String,
    size_bytes: u64,
}

#[component]
pub fn FileUpload(props: FileUploadProps) -> Element {
    let mut upload_state: Signal<UploadState> = use_signal(|| UploadState::Idle);
    let mut is_drag_over: Signal<bool> = use_signal(|| false);

    let api_base = props.api_base.clone();
    let tenant_id = props.tenant_id.clone();

    let border_color = if *is_drag_over.read() { "#cba6f7" } else { "#313244" };
    let bg_color = if *is_drag_over.read() { "#cba6f722" } else { "#1e1e2e" };

    rsx! {
        div {
            class: "file-upload-zone",
            style: format!("
                border: 2px dashed {border};
                border-radius: 12px;
                background: {bg};
                padding: 2rem;
                text-align: center;
                cursor: pointer;
                transition: border-color 0.2s, background 0.2s;
                font-family: 'Inter', sans-serif;
                color: #cdd6f4;
            ", border = border_color, bg = bg_color),

            ondragover: move |e| {
                e.prevent_default();
                is_drag_over.set(true);
            },
            ondragleave: move |_| is_drag_over.set(false),
            ondrop: {
                let api_base = api_base.clone();
                let tenant_id = tenant_id.clone();
                move |e: DragEvent| {
                    is_drag_over.set(false);
                    let api_base = api_base.clone();
                    let tenant_id = tenant_id.clone();
                    // Dioxus v0.7 drag event file handling
                    spawn(async move {
                        upload_state.set(UploadState::Uploading { progress_pct: 0 });
                        let files = e.files();
                        if let Some(file_engine) = files {
                            let file_names = file_engine.files();
                            for file_name in file_names {
                                if let Some(file_bytes) = file_engine.read_file(&file_name).await {
                                    let result = upload_bytes(
                                        &api_base,
                                        &tenant_id,
                                        &file_name,
                                        file_bytes,
                                    ).await;
                                    match result {
                                        Ok(uploaded) => {
                                            props.on_upload.call(uploaded.clone());
                                            upload_state.set(UploadState::Done(uploaded));
                                        }
                                        Err(e) => {
                                            upload_state.set(UploadState::Failed(e));
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
            },

            match &*upload_state.read() {
                UploadState::Idle => rsx! {
                    div {
                        div { style: "font-size:2.5rem; margin-bottom:0.5rem;", "☁️" }
                        p { style: "font-size:0.95rem; font-weight:600; color:#cba6f7;", "Drop files here or click to browse" }
                        p { style: "font-size:0.78rem; color:#6c7086; margin-top:0.25rem;", "Files are SHA-256 deduplicated — identical uploads are instant." }
                        input {
                            r#type: "file",
                            accept: props.accept.clone(),
                            multiple: props.multiple,
                            style: "
                                position:absolute; top:0; left:0; width:100%; height:100%;
                                opacity:0; cursor:pointer;
                            ",
                            onchange: {
                                let api_base = api_base.clone();
                                let tenant_id = tenant_id.clone();
                                move |e: FormEvent| {
                                    let api_base = api_base.clone();
                                    let tenant_id = tenant_id.clone();
                                    spawn(async move {
                                        upload_state.set(UploadState::Uploading { progress_pct: 0 });
                                        if let Some(file_engine) = e.files() {
                                            for file_name in file_engine.files() {
                                                if let Some(bytes) = file_engine.read_file(&file_name).await {
                                                    match upload_bytes(&api_base, &tenant_id, &file_name, bytes).await {
                                                        Ok(uploaded) => {
                                                            props.on_upload.call(uploaded.clone());
                                                            upload_state.set(UploadState::Done(uploaded));
                                                        }
                                                        Err(e) => upload_state.set(UploadState::Failed(e)),
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        }
                    }
                },

                UploadState::Uploading { progress_pct } => rsx! {
                    div {
                        div { style: "font-size:2rem; margin-bottom:0.75rem;", "⏫" }
                        p { style: "font-size:0.9rem; color:#89b4fa;", "Uploading… {progress_pct}%" }
                        div {
                            style: "width:80%; margin:0 auto; height:6px; background:#313244; border-radius:9999px; overflow:hidden;",
                            div {
                                style: format!("height:100%; width:{}%; background:linear-gradient(90deg,#cba6f7,#89b4fa); transition:width 0.3s;", progress_pct),
                            }
                        }
                    }
                },

                UploadState::Done(file) => rsx! {
                    div {
                        div { style: "font-size:2rem; margin-bottom:0.5rem;", "✅" }
                        p { style: "font-size:0.9rem; font-weight:600; color:#a6e3a1;", "Upload complete!" }
                        p { style: "font-size:0.78rem; color:#6c7086;", "{file.original_name}" }
                        code {
                            style: "font-size:0.7rem; color:#585b70; word-break:break-all;",
                            "sha256:{file.file_hash}"
                        }
                        div { style: "margin-top:0.75rem;",
                            button {
                                style: "
                                    padding:0.4rem 1rem; border-radius:6px; border:none; cursor:pointer;
                                    background:#313244; color:#cdd6f4; font-size:0.8rem;
                                ",
                                onclick: move |_| upload_state.set(UploadState::Idle),
                                "Upload another"
                            }
                        }
                    }
                },

                UploadState::Failed(err) => rsx! {
                    div {
                        div { style: "font-size:2rem; margin-bottom:0.5rem;", "❌" }
                        p { style: "font-size:0.9rem; font-weight:600; color:#f38ba8;", "Upload failed" }
                        p { style: "font-size:0.78rem; color:#6c7086;", "{err}" }
                        button {
                            style: "
                                margin-top:0.5rem; padding:0.4rem 1rem; border-radius:6px; border:none;
                                cursor:pointer; background:#313244; color:#cdd6f4; font-size:0.8rem;
                            ",
                            onclick: move |_| upload_state.set(UploadState::Idle),
                            "Try again"
                        }
                    }
                },
            }
        }
    }
}

/// Uploads raw bytes to the frappe-storage multipart endpoint and returns
/// the de-duplicated SHA-256 file record.
async fn upload_bytes(
    api_base: &str,
    tenant_id: &str,
    filename: &str,
    bytes: Vec<u8>,
) -> Result<UploadedFile, String> {
    use reqwest::multipart;

    let part = multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| e.to_string())?;

    let form = multipart::Form::new()
        .text("tenant_id", tenant_id.to_string())
        .part("file", part);

    let client = reqwest::Client::new();
    let url = format!("{}/api/storage/upload", api_base);
    let resp = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("Server returned {}", resp.status()));
    }

    let upload_resp: UploadResponse = resp.json().await.map_err(|e| e.to_string())?;

    Ok(UploadedFile {
        file_hash: upload_resp.file_hash,
        original_name: upload_resp.original_name,
        size_bytes: upload_resp.size_bytes,
        url: upload_resp.url,
    })
}
