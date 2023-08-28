CREATE TABLE spt_albums_covers (
  album_id VARCHAR NOT NULL PRIMARY KEY REFERENCES spt_albums(id) ,
  url VARCHAR NOT NULL,
  height INTEGER NOT NULL,
  width INTEGER NOT NULL
);
ALTER TABLE spt_songs 
  ADD album VARCHAR NOT NULL REFERENCES spt_albums(id)