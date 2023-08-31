use ratatui::{
    prelude::{Alignment, Buffer, Constraint, Direction, Layout, Rect},
    widgets::{Paragraph, Widget},
};

use crate::{DisplayTimestamp, SpotifyUi};

#[derive(Debug, Default, Clone, Eq, PartialEq, Hash)]
pub struct Clear(pub SpotifyUi);

impl Widget for Clear {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.area() == 0 {
            return;
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ]
                .as_ref(),
            )
            .split(area);

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

        let pretty_duration = format!(
            "{}",
            chrono::Duration::from_std(self.0.duration)
                .unwrap()
                .display_timestamp()
                .unwrap()
        );
        let title = Paragraph::new(format!(
            "Titre: {}\nDurée: {}",
            self.0.title, pretty_duration
        ))
        .alignment(Alignment::Center);
        title.render(chunks[0], buf);

        let title = Paragraph::new(format!(
            "Artiste: {}\nAlbum: {} ({})",
            self.0.artist, self.0.album_name, self.0.album_kind
        ))
        .alignment(Alignment::Center);
        title.render(chunks[2], buf);

        let title = Paragraph::new("P pour preview").alignment(Alignment::Center);
        title.render(chunks2[0], buf);

        let title =
            Paragraph::new("Y pour ouvrir dans Youtube Search").alignment(Alignment::Center);
        title.render(chunks2[1], buf);

        let title = Paragraph::new("Entrée pour aller sur la musique suivante")
            .alignment(Alignment::Center);
        title.render(chunks2[2], buf);

        let img = image::load_from_memory(&self.0.cover_img)
            .expect("Data from stdin could not be decoded.");
        let width = (area.width as f32 * 0.20_f32) as u32;
        let conf = viuer::Config {
            width: Some(width),
            x: area.x + (area.width as f32 * 0.5 - (width / 2) as f32) as u16,
            y: area.y as i16 + (area.height as f32 * 0.07) as i16,
            use_kitty: false,
            ..Default::default()
        };
        viuer::print(&img, &conf).expect("Image printing failed.");
    }
}
