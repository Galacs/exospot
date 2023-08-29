use anyhow::anyhow;
use bytes::Bytes;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode},
    execute,
    terminal::{
        self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use futures::stream::TryStreamExt;
use futures_util::{FutureExt, StreamExt, AsyncReadExt};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use rodio::{Source, Sink};
use rspotify::{
    model::{AlbumId, PlayableItem, PlaylistId},
    prelude::*,
    ClientCredsSpotify, Credentials,
};
use sqlx::SqliteConnection;
use sqlx::{sqlite::SqliteConnectOptions, Connection, SqlitePool};
use symphonia::core::io::MediaSource;
use std::{
    error::Error,
    io::{self, Stdout, Read},
    process::exit,
    sync::Arc,
    time::Duration,
};
use tokio::{select, sync::Mutex};

use viuer::{print, Config};

mod symphonia_decoder;

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>, Box<dyn Error>> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<(), Box<dyn Error>> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen,)?;
    Ok(terminal.show_cursor()?)
}

#[derive(Debug, Clone)]
enum App {
    Welcome,
    Spotify(SpotifyUi),
}

#[derive(Debug, Clone)]
struct SpotifyUi {
    title: String,
    artist: String,
    cover_img: Bytes,
    album_name: String,
    album_kind: String,
    duration: Duration,
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &App,
) -> Result<(), Box<dyn Error>> {
    terminal.draw(|frame| {
        match app {
            App::Welcome => {
                let greeting = Paragraph::new("Welcome to Exospot");
                frame.render_widget(greeting, frame.size());
            }
            App::Spotify(spt_ui) => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    // .margin(0)
                    .constraints(
                        [
                            Constraint::Percentage(25),
                            Constraint::Percentage(25),
                            Constraint::Percentage(25),
                            Constraint::Percentage(25),
                        ]
                        .as_ref(),
                    )
                    .split(frame.size());

                let chunks2 = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(
                        [
                            Constraint::Ratio(1, 4),
                            Constraint::Ratio(2, 4),
                            Constraint::Ratio(3, 4),
                        ]
                        .as_ref(),
                    )
                    .split(chunks[3]);

                let pretty_duration = format!("{}", chrono::Duration::from_std(spt_ui.duration).unwrap().display_timestamp().unwrap());
                let title =
                    Paragraph::new(format!("Titre: {}\nDurée: {}", spt_ui.title, pretty_duration)).alignment(Alignment::Center);
                frame.render_widget(title, chunks[1]);
                let title = Paragraph::new(format!(
                    "Artiste: {}\nAlbum: {} ({})",
                    spt_ui.artist, spt_ui.album_name, spt_ui.album_kind
                ))
                .alignment(Alignment::Center);
                frame.render_widget(title, chunks[2]);
                let title = Paragraph::new("P pour preview").alignment(Alignment::Center);
                frame.render_widget(title, chunks2[0]);
                let title = Paragraph::new("Y pour ouvrir dans Youtube Search")
                    .alignment(Alignment::Center);
                frame.render_widget(title, chunks2[1]);
                let title = Paragraph::new("Entrée pour aller sur la musique suivante")
                    .alignment(Alignment::Center);
                frame.render_widget(title, chunks2[2]);

                let img = image::load_from_memory(&spt_ui.cover_img)
                    .expect("Data from stdin could not be decoded.");
                let conf = Config {
                    width: Some(20),
                    height: Some(10),
                    x: 10,
                    y: 4,
                    use_kitty: false,
                    ..Default::default()
                };
                print(&img, &conf).expect("Image printing failed.");
            }
        }
    })?;
    Ok(())
}

trait DisplayTimestamp {
    fn display_timestamp(&self) -> Result<String, anyhow::Error>;
}

impl DisplayTimestamp for chrono::Duration {
    fn display_timestamp(&self) -> Result<String, anyhow::Error> {
        let mut a = chrono::Duration::from(*self);
        let minutes = a.num_minutes();
        a = a - chrono::Duration::from_std(std::time::Duration::from_secs((a.num_minutes()*60) as u64))?;
        let seconds = a.num_seconds();
        Ok(format!("{minutes:0>2}:{seconds:0>2}"))
    }
}

async fn input(
    tx: tokio::sync::mpsc::Sender<Event>,
    update_tx: tokio::sync::watch::Sender<bool>,
) {
    let mut reader = EventStream::new();
    loop {
        let event = reader.next().await;
        let Some(event) = event else { continue };
        let Ok(event) = event else { continue };
        match event {
            Event::FocusGained => {}
            Event::FocusLost => {}
            Event::Key(key) => match key.code {
                KeyCode::Char('q') => break,
                _ => {}
            },
            Event::Mouse(_) => {}
            Event::Paste(_) => {}
            Event::Resize(_, _) => {
                update_tx.send(true).unwrap();
            }
        }
        tx.send(event).await.unwrap();
    }
}

async fn ui(
    term: Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>,
    mut rx: tokio::sync::watch::Receiver<App>,
    mut update_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut state = rx.borrow().to_owned();
    loop {
        select! {
            _ = rx.changed() => state = rx.borrow().to_owned(),
            _ = update_rx.changed() => {},
        }
        let mut terminal = term.lock().await;
        draw(&mut terminal, &state).unwrap();
    }
}

#[derive(Debug, Clone, Copy)]
enum StreamStatus {
    Play,
}

async fn stream_and_play_mp3(mp3_url: String, mut rx: tokio::sync::watch::Receiver<StreamStatus>, stream_handle: rodio::OutputStreamHandle) {
    struct Reader<R>(futures_util::io::BufReader<R>);

    impl<R: futures_util::AsyncRead + std::marker::Unpin> Read for Reader<R> {
        fn read(&mut self, mut buf: &mut [u8]) -> io::Result<usize> {
            use futures::executor;
            executor::block_on(async {
                self.0.read(&mut buf).await
            })
        }
    }
    impl<R: futures_util::AsyncRead> std::io::Seek for Reader<R> {
        fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
            unimplemented!()
        }
    }

    impl<R: futures_util::AsyncRead + std::marker::Unpin + std::marker::Send + std::marker::Sync> MediaSource for Reader<R> {
        fn is_seekable(&self) -> bool {
            false
        }

        fn byte_len(&self) -> Option<u64> {
            None
        }
    }
    
    use symphonia::core::io::MediaSourceStream;

    let sink = Sink::try_new(&stream_handle).unwrap();
    while rx.changed().await.is_ok() {
        let status = rx.borrow().clone();
        match status {
            StreamStatus::Play => {
                if !sink.empty() {
                    sink.stop();
                    continue
                }
                let response = reqwest::get(&mp3_url).await.unwrap();
                let stream = response.bytes_stream().map_err(|e| futures::io::Error::new(futures::io::ErrorKind::Other, e)).into_async_read();
                let reader = Reader(futures_util::io::BufReader::new(stream));
                let mss = MediaSourceStream::new(Box::new(reader), Default::default());
                let decoder = tokio::task::spawn_blocking(|| {
                    symphonia_decoder::SymphoniaDecoder::new(mss, Some("mp3")).unwrap()
                }).await.unwrap();
                sink.append(decoder);
            },
        }
    }
}

#[tokio::main]
async fn main() {
    // Restore terminal on panic
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut terminal = setup_terminal().unwrap();
        default_panic(info);
        restore_terminal(&mut terminal).unwrap();
    }));

    // TUI
    let  terminal = setup_terminal().unwrap();
    let  app_state: App = App::Welcome;
    let (tx, rx) = tokio::sync::watch::channel(app_state.clone());
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(8);
    let (update_tx, update_rx) = tokio::sync::watch::channel(true);
    let terminal = Arc::new(Mutex::new(terminal));
    let task = tokio::task::spawn(ui(terminal.clone(), rx, update_rx));
    let input_task = tokio::task::spawn(input(input_tx, update_tx));

    let term = terminal.clone();
    tokio::spawn(async move {
        select! {
            _ = task => {},
            _ = input_task => {}
        }
        let mut terminal = term.lock().await;
        restore_terminal(&mut terminal).unwrap();
        exit(0);
    });

    // SQL pool
    let database_url = "sqlite://songs.db";
    let conn = SqlitePool::connect(database_url).await.unwrap();
    sqlx::migrate!().run(&conn).await.unwrap();
    // sync_from_spotify(&conn).await;


    let spt_songs = sqlx::query!("select * from spt_songs ORDER BY RANDOM()")
        .fetch_all(&conn)
        .await;
    for song in spt_songs.unwrap() {
        let artists = sqlx::query!(
            "SELECT spt_artists.name, spt_artists.id
            FROM spt_songs_spt_artists
            INNER JOIN spt_songs ON spt_songs_spt_artists.spt_song_id = spt_songs.id
            INNER JOIN spt_artists ON spt_songs_spt_artists.spt_artist_id = spt_artists.id
            WHERE spt_song_id = ?",
            song.id
        )
        .fetch_all(&conn)
        .await
        .unwrap();
        let album = sqlx::query!("SELECT * FROM spt_albums WHERE id = ?", song.album)
            .fetch_one(&conn)
            .await
            .unwrap();
        let image = sqlx::query!(
            "SELECT URL FROM spt_albums_covers WHERE album_id = ? ORDER BY height DESC",
            song.album
        )
        .fetch_one(&conn)
        .await
        .unwrap();
        let img_buf = reqwest::get(image.url)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let app_state = App::Spotify(SpotifyUi {
            title: song.title.to_owned(),
            artist: song.artist.to_owned(),
            cover_img: img_buf,
            album_name: album.name.to_owned(),
            album_kind: album.kind.to_owned(),
            duration: Duration::from_millis(song.duration as u64)
        });
        let url = song.preview_url;
        tx.send(app_state).unwrap();

        

        let (preview_tx, preview_rx) = tokio::sync::watch::channel(StreamStatus::Play);
        let (_stream, stream_handle) = rodio::OutputStream::try_default().unwrap();
        if let Some(url) = url.clone() {
            tokio::task::spawn(stream_and_play_mp3(url, preview_rx, stream_handle));
        }

        'outer: loop {
            select! {
                Some(msg) = input_rx.recv() => {
                    let Event::Key(key) = msg else { continue };
                    match key.code {
                        KeyCode::Enter => break 'outer,
                        KeyCode::Char('y') => { open::that(format!("https://www.youtube.com/results?search_query={}", urlencoding::encode(&format!("{} {}", song.artist, song.title))).to_string()).unwrap(); }
                        KeyCode::Char('p') => { if let Some(_) = url { preview_tx.send(StreamStatus::Play).unwrap() }}
                        _ => {}
                    }
                }
            }
        }
        // println!("Titre: {}", song.title);
        // println!("Artiste: {}", song.artist);
        // println!("Artistes: {:?}", artists);
        // println!("ID: {}", song.id);
        // println!("Album: {}", song.album);
        // println!("Album name: {}", album.name);
        // println!("Album type: {}", album.kind);

        // println!("Biggest cover image url: {}", image.url);
    }

    conn.close().await;
    let mut terminal = terminal.lock().await;
    restore_terminal(&mut terminal).unwrap();
}

async fn sync_from_spotify(conn: &sqlx::SqlitePool) {
    let creds = Credentials::from_env().unwrap();
    let spotify = ClientCredsSpotify::new(creds);
    spotify.request_token().await.unwrap();

    let playlist = spotify.playlist_items(
        PlaylistId::from_id("2qv1rmsLVKtnk3n9oLj3vb").unwrap(),
        None,
        None,
    );

    // let mut ids = std::collections::HashSet::new();
    // let data = std::sync::Arc::new(std::sync::Mutex::new(ids));

    playlist.try_for_each_concurrent(10, |item| async {
        if let Some(playable) = item.track {
            if let PlayableItem::Track(track) = playable {
                // dbg!(&track);
                let id = track.id.clone().unwrap().id().to_owned();
                let title = track.name.to_owned();
                let artist = track.artists.first().unwrap().name.to_owned();
                let album_id = track.album.id.unwrap().to_string();
                let album_type = track.album.album_type.unwrap();
                let duration_ms = track.duration.num_milliseconds();
                let preview_url = track.preview_url;

                if let Ok(_) = sqlx::query!("INSERT INTO spt_albums(id, name, kind) VALUES ($1, $2, $3)", album_id, track.album.name, album_type).execute(conn).await {
                    for image in &track.album.images {
                        sqlx::query!("INSERT INTO spt_albums_covers(album_id, url, height, width) VALUES ($1, $2, $3, $4)",
                        album_id, image.url, image.height, image.width).execute(conn).await;
                    }
                }
                if let Ok(_) = sqlx::query!(
                    "INSERT INTO spt_songs(id, title, artist, album, duration, preview_url) VALUES ($1, $2, $3, $4, $5, $6)",
                    id,
                    title,
                    artist,
                    album_id,
                    duration_ms,
                    preview_url
                ).execute(conn).await {
                    for i in &track.artists {
                        let a = &i.id.clone().unwrap().to_string();
                        sqlx::query!(
                            "INSERT INTO spt_artists(id, name) VALUES ($1, $2)",
                            a,
                            i.name
                        )
                        .execute(conn)
                        .await;
                        sqlx::query!("INSERT INTO spt_songs_spt_artists(spt_song_id, spt_artist_id) VALUES ($1, $2)", id, a).execute(conn).await;
                    }
                }
                // let mut ids = data.lock().unwrap();
                // if !ids.insert(title.to_owned()) {
                //     println!("{}    {}      {}", id, title, artist)
                // }
            }
        }
        Ok(())
    }).await.unwrap();
}
