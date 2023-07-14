use rspotify::{
    model::{AlbumId, PlayableItem, PlaylistId},
    prelude::*,
    ClientCredsSpotify, Credentials,
};
use sqlx::SqliteConnection;
use sqlx::{sqlite::SqliteConnectOptions, Connection, SqlitePool};
use futures::stream::TryStreamExt;

#[tokio::main]
async fn main() {
    // SQL pool
    let database_url = "sqlite://songs.db";
    let mut conn = SqlitePool::connect(database_url).await.unwrap();
    sqlx::migrate!().run(&conn).await.unwrap();

    sync_from_spotify(&conn).await;

    let results = sqlx::query!("select id from spt_artists")
        .fetch_all(&conn)
        .await;
    dbg!(results);

    let results = sqlx::query!("select id from spt_songs")
        .fetch_all(&conn)
        .await;
    dbg!(results);

    let results = sqlx::query!("SELECT * from spt_songs_spt_artists")
        .fetch_all(&conn)
        .await;
    dbg!(results);

    let results = sqlx::query!("SELECT name
        FROM spt_songs_spt_artists
        INNER JOIN spt_songs ON spt_songs_spt_artists.spt_song_id = spt_songs.id
        INNER JOIN spt_artists ON spt_songs_spt_artists.spt_artist_id = spt_artists.id
        WHERE spt_song_id = '1HVKbxwcF6VeP7n9CBzO9k'")
        .fetch_all(&conn)
        .await;
    dbg!(results);

    conn.close().await;
}

async fn sync_from_spotify(conn: &sqlx::SqlitePool) {
    let creds = Credentials::from_env().unwrap();
    let spotify = ClientCredsSpotify::new(creds);
    spotify.request_token().await.unwrap();

    let playlist = spotify
        .playlist_items(
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
                if let Ok(_) = sqlx::query!("INSERT INTO spt_albums(id, name) VALUES ($1, $2)", album_id, track.album.name).execute(conn).await {
                    for image in &track.album.images {
                        sqlx::query!("INSERT INTO spt_albums_covers(album_id, url, height, width) VALUES ($1, $2, $3, $4)",
                        album_id, image.url, image.height, image.width).execute(conn).await;
                    }
                }
                sqlx::query!(
                    "INSERT INTO spt_songs(id, title, artist, album) VALUES ($1, $2, $3, $4)",
                    id,
                    title,
                    artist,
                    album_id
                )
                .execute(conn)
                .await;
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
        Ok(())
    }).await.unwrap();
}