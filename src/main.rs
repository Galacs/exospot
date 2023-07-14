use bytes::Bytes;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, self},
};
use futures::stream::TryStreamExt;
use futures_util::StreamExt;
use rspotify::{
    model::{AlbumId, PlayableItem, PlaylistId},
    prelude::*,
    ClientCredsSpotify, Credentials,
};
use sqlx::SqliteConnection;
use sqlx::{sqlite::SqliteConnectOptions, Connection, SqlitePool};
use std::{io::{self, Stdout}, error::Error, time::Duration};
use ratatui::{backend::CrosstermBackend, Terminal, widgets::{Paragraph, Block, Borders}, layout::{Constraint, Layout, Direction, Alignment}, text::{Spans, Span}, style::{Style, Color, Modifier}};

use viuer::{print, Config};

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

#[derive(Debug)]
enum App {
    Welcome,
    Spotify(SpotifyUi),
}

#[derive(Debug)]
struct SpotifyUi {
    title: String,
    artist: String,
    cover_img: Bytes,
    album_name: String,
    album_kind: String
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &App) -> Result<Option<KeyCode>, Box<dyn Error>> {
    loop {
        terminal.draw(|frame| {
            match app {
                App::Welcome => {
                    let greeting = Paragraph::new("Welcome to Exospot");
                    frame.render_widget(greeting, frame.size());
                },
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
                        .constraints([Constraint::Ratio(1, 4), Constraint::Ratio(2, 4), Constraint::Ratio(3, 4)].as_ref())
                        .split(chunks[3]);
                
                
                    let title = Paragraph::new(format!("Titre: {}", spt_ui.title)).alignment(Alignment::Center);
                    frame.render_widget(title, chunks[1]);
                    let title = Paragraph::new(format!("Artiste: {}\nAlbum: {} ({})", spt_ui.artist, spt_ui.album_name, spt_ui.album_kind)).alignment(Alignment::Center);
                    frame.render_widget(title, chunks[2]);
                    let title = Paragraph::new("Y pour faire ca").alignment(Alignment::Center);
                    frame.render_widget(title, chunks2[0]);
                    let title = Paragraph::new("Y pour ouvrir dans Youtube Search").alignment(Alignment::Center);
                    frame.render_widget(title, chunks2[1]);
                    let title = Paragraph::new("Entrée pour aller sur la musique suivante").alignment(Alignment::Center);
                    frame.render_widget(title, chunks2[2]);
                    
                    let img = image::load_from_memory(&spt_ui.cover_img).expect("Data from stdin could not be decoded.");
                    let conf = Config {
                        width: Some(50),
                        height: Some(50),
                        x: 10,
                        y: 4,
                        ..Default::default()
                    };
                    // print(&img, &conf).expect("Image printing failed.");
                },
            }
            
        })?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => {
                        restore_terminal(terminal).unwrap();
                        panic!();
                    },
                    _ => {
                        return Ok(Some(key.code))
                    }
                }
                
            }
        }
    }
    Ok(None)
}

fn pause() {
    use std::io::prelude::*;
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();

    // We want the cursor to stay at the end of the line, so we print without a newline and flush manually.
    write!(stdout, "Press any key to continue...").unwrap();
    stdout.flush().unwrap();

    // Read a single byte and discard
    let _ = stdin.read(&mut [0u8]).unwrap();
}

#[tokio::main]
async fn main() {
    // TUI
    let mut terminal = setup_terminal().unwrap();
    run(&mut terminal, &App::Welcome).unwrap();

    // SQL pool
    let database_url = "sqlite://songs.db";
    let mut conn = SqlitePool::connect(database_url).await.unwrap();
    sqlx::migrate!().run(&conn).await.unwrap();

    // sync_from_spotify(&conn).await;

    // let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    

    let spt_songs = sqlx::query!("select * from spt_songs").fetch_all(&conn).await;
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
        ).fetch_one(&conn).await.unwrap();
        // rx2.recv().await.unwrap();
        // tx.send(App::Spotify(SpotifyUi { title: song.title, artist: song.artist })).unwrap();
        let mut buf = reqwest::get(image.url).await.unwrap().bytes().await.unwrap();
        loop {   
            if let Some(key) = run(&mut terminal, &App::Spotify(SpotifyUi {title:song.title.to_owned(),artist:song.artist.to_owned(),cover_img:buf.to_owned(), album_name: album.name.to_owned(), album_kind: album.kind.to_owned() })).unwrap() {
                match key {
                    KeyCode::Enter => break,
                    KeyCode::Char('y') => {
                        open::that(format!("https://www.youtube.com/results?search_query={}", urlencoding::encode(&format!("{} {}", song.artist, song.title))).to_string());
                    }
                    _ => {}
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

        // pause();
    }

    // let results = sqlx::query!("select id from spt_artists")
    //     .fetch_all(&conn)
    //     .await;
    // dbg!(results);

    // let results = sqlx::query!("select id from spt_songs")
    //     .fetch_all(&conn)
    //     .await;
    // dbg!(results);

    // let results = sqlx::query!("SELECT * from spt_songs_spt_artists")
    //     .fetch_all(&conn)
    //     .await;
    // dbg!(results);

    // let results = sqlx::query!("SELECT name
    //     FROM spt_songs_spt_artists
    //     INNER JOIN spt_songs ON spt_songs_spt_artists.spt_song_id = spt_songs.id
    //     INNER JOIN spt_artists ON spt_songs_spt_artists.spt_artist_id = spt_artists.id
    //     WHERE spt_song_id = '1HVKbxwcF6VeP7n9CBzO9k'")
    //     .fetch_all(&conn)
    //     .await;
    // dbg!(results);

    conn.close().await;
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

    playlist.try_for_each_concurrent(10, |item| async {
        if let Some(playable) = item.track {
            if let PlayableItem::Track(track) = playable {
                // dbg!(&track);
                let id = track.id.clone().unwrap().id().to_owned();
                let title = track.name.to_owned();
                let artist = track.artists.first().unwrap().name.to_owned();
                let album_id = track.album.id.unwrap().to_string();
                let album_type = track.album.album_type.unwrap();
                if let Ok(_) = sqlx::query!("INSERT INTO spt_albums(id, name, kind) VALUES ($1, $2, $3)", album_id, track.album.name, album_type).execute(conn).await {
                    for image in &track.album.images {
                        sqlx::query!("INSERT INTO spt_albums_covers(album_id, url, height, width) VALUES ($1, $2, $3, $4)",
                        album_id, image.url, image.height, image.width).execute(conn).await;
                    }
                }
                if let Ok(_) = sqlx::query!(
                    "INSERT INTO spt_songs(id, title, artist, album) VALUES ($1, $2, $3, $4)",
                    id,
                    title,
                    artist,
                    album_id
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
            }
        }
        Ok(())
    }).await.unwrap();
}
