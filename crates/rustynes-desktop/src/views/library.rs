//! ROM library browser view.
//!
//! Displays discovered ROMs in grid or list format with search and filtering.

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, Space},
    Alignment, Element, Length,
};

use crate::library::{LibraryState, ViewMode};
use crate::message::Message;

/// Render the library view
pub fn view(library: &LibraryState) -> Element<'_, Message> {
    let header = create_header(library);
    let content = match library.view_mode {
        ViewMode::Grid => create_grid_view(library),
        ViewMode::List => create_list_view(library),
    };

    column![header, content]
        .spacing(10)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Create library header with search and controls
fn create_header(library: &LibraryState) -> Element<'_, Message> {
    let search_input = text_input("Search ROMs...", &library.search_query)
        .on_input(Message::LibrarySearch)
        .padding(10)
        .width(Length::FillPortion(3));

    let view_toggle = button(match library.view_mode {
        ViewMode::Grid => text("List View"),
        ViewMode::List => text("Grid View"),
    })
    .on_press(Message::ToggleLibraryView)
    .padding(10);

    let select_dir = button(text("Select ROM Directory"))
        .on_press(Message::SelectRomDirectory)
        .padding(10);

    let rom_count = text(format!("{} ROMs", library.rom_count())).size(14);

    row![
        search_input,
        Space::with_width(10),
        view_toggle,
        Space::with_width(10),
        select_dir,
        Space::with_width(20),
        rom_count,
    ]
    .align_y(Alignment::Center)
    .into()
}

/// Create grid view (4 columns)
fn create_grid_view(library: &LibraryState) -> Element<'_, Message> {
    let roms = library.filtered_roms();

    if roms.is_empty() {
        return create_empty_state(library);
    }

    let mut grid = column![].spacing(15).padding(10);
    let mut current_row = row![].spacing(15);
    let mut count = 0;

    for rom in roms {
        current_row = current_row.push(create_rom_card(rom));
        count += 1;

        if count % 4 == 0 {
            grid = grid.push(current_row);
            current_row = row![].spacing(15);
        }
    }

    // Add remaining items
    if count % 4 != 0 {
        // Fill remaining slots with empty space for alignment
        while count % 4 != 0 {
            current_row = current_row.push(Space::with_width(170));
            count += 1;
        }
        grid = grid.push(current_row);
    }

    scrollable(container(grid).width(Length::Fill).height(Length::Fill)).into()
}

/// Create a ROM card for grid view
fn create_rom_card(rom: &crate::library::scanner::RomEntry) -> Element<'_, Message> {
    let rom_path = rom.path.clone();

    let card_content = column![
        // Placeholder for cover art (future enhancement)
        container(text("🎮").size(64))
            .width(Length::Fixed(150.0))
            .height(Length::Fixed(150.0))
            .center_x(150.0)
            .center_y(150.0)
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(iced::Color::from_rgb(0.15, 0.15, 0.2).into()),
                border: iced::Border {
                    color: iced::Color::from_rgb(0.3, 0.3, 0.4),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }),
        Space::with_height(10),
        // ROM title
        text(&rom.title).size(14).width(Length::Fixed(150.0)),
        // File size
        text(rom.size_display())
            .size(11)
            .width(Length::Fixed(150.0))
            .style(|_theme: &iced::Theme| text::Style {
                color: Some(iced::Color::from_rgb(0.6, 0.6, 0.7)),
            }),
    ]
    .align_x(Alignment::Center)
    .spacing(5);

    button(card_content)
        .on_press(Message::LoadRom(rom_path))
        .padding(10)
        .style(|theme: &iced::Theme, status| {
            let palette = theme.extended_palette();
            match status {
                button::Status::Active => button::Style {
                    background: Some(iced::Color::from_rgb(0.1, 0.1, 0.15).into()),
                    border: iced::Border {
                        color: iced::Color::from_rgb(0.2, 0.2, 0.3),
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    text_color: palette.background.base.text,
                    ..Default::default()
                },
                button::Status::Hovered => button::Style {
                    background: Some(iced::Color::from_rgb(0.15, 0.15, 0.25).into()),
                    border: iced::Border {
                        color: iced::Color::from_rgb(0.4, 0.4, 0.6),
                        width: 2.0,
                        radius: 8.0.into(),
                    },
                    text_color: palette.primary.strong.text,
                    ..Default::default()
                },
                button::Status::Pressed => button::Style {
                    background: Some(iced::Color::from_rgb(0.2, 0.2, 0.3).into()),
                    border: iced::Border {
                        color: iced::Color::from_rgb(0.5, 0.5, 0.7),
                        width: 2.0,
                        radius: 8.0.into(),
                    },
                    text_color: palette.primary.strong.text,
                    ..Default::default()
                },
                button::Status::Disabled => button::Style::default(),
            }
        })
        .into()
}

/// Create list view
fn create_list_view(library: &LibraryState) -> Element<'_, Message> {
    let roms = library.filtered_roms();

    if roms.is_empty() {
        return create_empty_state(library);
    }

    let mut list = column![].spacing(2);

    // Header row
    list = list.push(
        container(
            row![
                text("Title").size(14).width(Length::FillPortion(4)),
                text("Size").size(14).width(Length::FillPortion(1)),
                text("Mapper").size(14).width(Length::FillPortion(2)),
            ]
            .padding(10)
            .spacing(10),
        )
        .style(|_theme: &iced::Theme| container::Style {
            background: Some(iced::Color::from_rgb(0.15, 0.15, 0.2).into()),
            border: iced::Border {
                color: iced::Color::from_rgb(0.3, 0.3, 0.4),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }),
    );

    // ROM entries
    for rom in roms {
        list = list.push(create_list_row(rom));
    }

    scrollable(container(list).width(Length::Fill).height(Length::Fill)).into()
}

/// Create a list row for a ROM
fn create_list_row(rom: &crate::library::scanner::RomEntry) -> Element<'_, Message> {
    let rom_path = rom.path.clone();

    let row_content = row![
        text(&rom.title).size(14).width(Length::FillPortion(4)),
        text(rom.size_display())
            .size(14)
            .width(Length::FillPortion(1)),
        text(rom.mapper_display())
            .size(14)
            .width(Length::FillPortion(2)),
    ]
    .padding(10)
    .spacing(10)
    .align_y(Alignment::Center);

    button(row_content)
        .on_press(Message::LoadRom(rom_path))
        .width(Length::Fill)
        .style(|theme: &iced::Theme, status| {
            let palette = theme.extended_palette();
            match status {
                button::Status::Active => button::Style {
                    background: Some(iced::Color::from_rgb(0.08, 0.08, 0.12).into()),
                    border: iced::Border {
                        color: iced::Color::from_rgb(0.2, 0.2, 0.3),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    text_color: palette.background.base.text,
                    ..Default::default()
                },
                button::Status::Hovered => button::Style {
                    background: Some(iced::Color::from_rgb(0.12, 0.12, 0.18).into()),
                    border: iced::Border {
                        color: iced::Color::from_rgb(0.4, 0.4, 0.6),
                        width: 2.0,
                        radius: 4.0.into(),
                    },
                    text_color: palette.primary.strong.text,
                    ..Default::default()
                },
                button::Status::Pressed => button::Style {
                    background: Some(iced::Color::from_rgb(0.15, 0.15, 0.22).into()),
                    border: iced::Border {
                        color: iced::Color::from_rgb(0.5, 0.5, 0.7),
                        width: 2.0,
                        radius: 4.0.into(),
                    },
                    text_color: palette.primary.strong.text,
                    ..Default::default()
                },
                button::Status::Disabled => button::Style::default(),
            }
        })
        .into()
}

/// Create empty state message
fn create_empty_state(library: &LibraryState) -> Element<'_, Message> {
    let message = if library.rom_directory.is_none() {
        "No ROM directory selected.\nClick 'Select ROM Directory' to choose a folder containing .nes files."
    } else if library.search_query.is_empty() {
        "No ROMs found in the selected directory."
    } else {
        "No ROMs match your search query."
    };

    container(
        column![
            text("📂").size(64),
            Space::with_height(20),
            text(message)
                .size(16)
                .style(|_theme: &iced::Theme| text::Style {
                    color: Some(iced::Color::from_rgb(0.6, 0.6, 0.7)),
                }),
        ]
        .align_x(Alignment::Center)
        .spacing(10),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}
