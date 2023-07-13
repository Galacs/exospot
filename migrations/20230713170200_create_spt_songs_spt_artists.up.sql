CREATE TABLE spt_songs_spt_artists (
  spt_song_id VARCHAR REFERENCES spt_songs(id),
  spt_artist_id VARCHAR REFERENCES spt_artists(id),
  PRIMARY KEY(spt_song_id, spt_artist_id)
)