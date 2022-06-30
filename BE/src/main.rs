use axum::{
    body::{Bytes, StreamBody},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ContentLengthLimit, Extension, Multipart,
    },
    // http::Method,
    http::{header, StatusCode},
    response::Html,
    response::IntoResponse,
    routing::{get, get_service, post},
    BoxError,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use futures::{sink::SinkExt, stream::StreamExt, Stream, TryStreamExt};
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::mpsc;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tower_http::services::ServeDir;
// use tokio::sync::mpsc;

use tokio::sync::broadcast;
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncWriteExt,
    io::BufWriter,
};
use tokio_util::io::ReaderStream;
use tokio_util::io::StreamReader;
// use tower_http::cors::{CorsLayer, Origin};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// use std::collections::VecDeque;
use std::thread;
use std::time::Duration;

struct BinarySlice {
    filename: String,
    data: Vec<u8>,
    header: Option<String>,
}

struct AppState {
    user_set: Mutex<HashSet<String>>,
    tx: broadcast::Sender<String>,
    data_queue: Mutex<Vec<BinarySlice>>,
}

const MAX_WRITE_SIZE: usize = 15 * 1024;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "example_templates=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let (tx, _rx) = broadcast::channel(100);
    let user_set = Mutex::new(HashSet::new());
    let data_queue = Mutex::new(Vec::<BinarySlice>::new());
    let app_state = Arc::new(AppState {
        user_set,
        tx: tx.clone(),
        data_queue,
    });

    let app_state1 = Arc::clone(&app_state);
    let (webm_sender, webm_receiver) = mpsc::channel::<BinarySlice>();

    tokio::spawn(async move {
        loop {
            let mut queue = app_state1.data_queue.lock().unwrap();
            if queue.len() > 0 {
                // println!("Vec size: {}", queue.len());
                let mut slice = queue.remove(0);

                if slice.data.len() > MAX_WRITE_SIZE {
                    let tail = slice.data.drain(MAX_WRITE_SIZE..).collect();
                    let filename = slice.filename.clone();
                    queue.insert(
                        0,
                        BinarySlice {
                            filename: filename,
                            data: tail,
                            header: slice.header.clone(),
                        },
                    );
                }
                webm_sender.send(slice).unwrap();

                thread::sleep(Duration::from_millis(10));
            } else {
                // println!("Empty");
                thread::sleep(Duration::from_millis(1000));
            }

            drop(queue);

            thread::sleep(Duration::from_millis(10));
        }
    });

    tokio::spawn(async move {
        loop {
            let slice = webm_receiver.recv().unwrap();

            if let Some(msg) = slice.header {
                println!("Msg in queue: {}", msg);
                if msg == "stop" {
                    // println!("sending...");

                    match tx.send(String::from(slice.filename)) {
                        Ok(obj) => {
                            println!("Sent back to clients: {}", obj);
                        }
                        Err(_) => {
                            println!("Err");
                        }
                    }
                    // println!("res: {}", _send);
                }
            } else {
                // println!(
                //     "Writing to file {} : {} bytes",
                //     slice.filename,
                //     slice.data.len()
                // );
                let _ = write_binary_file(slice.data, slice.filename).await;
            }

            thread::sleep(Duration::from_millis(10));
        }
    });

    let app_state2 = Arc::clone(&app_state);

    let app = Router::new()
        .route("/", get(index))
        .route("/download/:filename", get(download))
        .route("/api-file-upload", post(upload))
        // .layer(
        //     CorsLayer::new()
        //         .allow_origin(Origin::exact("http://localhost:3000".parse().unwrap()))
        //         .allow_methods(vec![Method::GET, Method::POST]),
        // )
        .route("/websocket", get(websocket_handler))
        .nest(
            "/static",
            get_service(ServeDir::new(".")).handle_error(|error: std::io::Error| async move {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Unhandled internal error: {}", error),
                )
            }),
        )
        .layer(Extension(app_state2));

    let config = RustlsConfig::from_pem_file("src/cert/cert.pem", "src/cert/key.rsa")
        .await
        .unwrap();

    let addr = SocketAddr::from(([0, 0, 0, 0], 4000));
    tracing::debug!("listening on {}", addr);
    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    Extension(state): Extension<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    // By splitting we can send and receive at the same time.
    let (mut sender, mut receiver) = stream.split();

    // Username gets set in the receive loop, if it's valid.
    let mut username = String::new();
    // Loop until a text message is found.
    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(name) = message {
            // If username that is sent by client is not taken, fill username string.
            check_username(&state, &mut username, &name);

            // If not empty we want to quit the loop else we want to quit function.
            if !username.is_empty() {
                break;
            } else {
                // Only send our client that username is taken.
                let _ = sender
                    .send(Message::Text(String::from("Username already taken.")))
                    .await;

                return;
            }
        }
    }

    // Subscribe before sending joined message.
    let mut rx = state.tx.subscribe();

    // Send joined message to all subscribers.
    let msg = format!("{} joined.", username);
    tracing::debug!("{}", msg);
    println!("msg: {}", msg.clone());
    let _ = state.tx.send(msg);

    // This task will receive broadcast messages and send text message to our client.
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // In any websocket error, break loop.
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Clone things we want to pass to the receiving task.
    let tx = state.tx.clone();
    let name = username.clone();
    let stt = Arc::clone(&state);

    // This task will receive messages from client and send them to broadcast subscribers.
    let mut recv_task = tokio::spawn(async move {
        while let Some(rcv_data) = receiver.next().await {
            if let Ok(Message::Binary(text)) = rcv_data {
                println!("New data received from {}, {} bytes", name, text.len());
                push_to_queue(&stt, text, &name);
            } else if let Ok(Message::Text(text)) = rcv_data {
                println!("Text Message from {} : {}", name, text);
                push_msg_to_queue(&stt, text, &name);
            }
        }
    });

    // If any one of the tasks exit, abort the other.
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    // Send user left message.
    let msg = format!("{} left.", username);
    // tracing::debug!("{}", msg);
    println!("{}", msg);
    let _ = tx.send(msg);
    // Remove username from map so new clients can take it.
    state.user_set.lock().unwrap().remove(&username);
}

fn push_to_queue(state: &AppState, data: Vec<u8>, name: &String) {
    let mut queue = state.data_queue.lock().unwrap();
    let mut filename = name.clone();
    filename.push_str(".webm");
    queue.push(BinarySlice {
        data,
        filename,
        header: None,
    });
}

fn push_msg_to_queue(state: &AppState, data: String, name: &String) {
    let mut queue = state.data_queue.lock().unwrap();
    let mut filename = name.clone();
    filename.push_str(".webm");
    queue.push(BinarySlice {
        data: Vec::<u8>::new(),
        filename,
        header: Some(data),
    });
}

async fn write_binary_file(data: Vec<u8>, filename: String) -> std::io::Result<()> {
    // let file = filename.clone();

    let path = format!("./uploads/{}", filename);
    // let path = filename;
    let mut file: tokio::fs::File;
    if Path::new(&path).exists() {
        file = OpenOptions::new()
            .append(true)
            .write(true)
            .open(&path)
            .await?;
    } else {
        file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await?;
    }

    let ret = file.write(&data).await;
    if let Ok(_size) = ret {
        // println!("Original: {}, Wrote {} bytes", data.len(), size);
    } else {
        println!("Something's went wrong while writing : {}", filename);
    }

    Ok(())
}

fn check_username(state: &AppState, string: &mut String, name: &str) {
    let mut user_set = state.user_set.lock().unwrap();

    if !user_set.contains(name) {
        user_set.insert(name.to_owned());

        string.push_str(name);
    }
}

async fn download(axum::extract::Path(filename): axum::extract::Path<String>) -> impl IntoResponse {
    println!("Download: {}", filename);

    let path = format!("{}.webm", filename);
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    };
    let stream = ReaderStream::new(file);
    let body = StreamBody::new(stream);

    let headers = [
        (header::CONTENT_TYPE, "text/toml; charset=utf-8"),
        (
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"1.webm\"",
        ),
    ];

    Ok((headers, body))
}

pub async fn upload(
    ContentLengthLimit(mut multipart): ContentLengthLimit<Multipart, { 2500 * 1024 * 1024 }>,
) {
    while let Some(field) = multipart.next_field().await.unwrap() {
        let _res = stream2file(field).await;
    }
}

async fn stream2file<S, E>(stream: S) -> Result<(), io::Error>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Into<BoxError>,
{
    // Convert the stream into an `AsyncRead`.
    let body_with_io_error = stream.map_err(|err| io::Error::new(io::ErrorKind::Other, err));
    let body_reader = StreamReader::new(body_with_io_error);
    futures::pin_mut!(body_reader);

    // Create the file. `File` implements `AsyncWrite`.
    let mut file = BufWriter::new(File::create("file.bin").await?);

    // Copy the body into the file.
    tokio::io::copy(&mut body_reader, &mut file).await?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(std::include_str!("../assets/index.html"))
}
