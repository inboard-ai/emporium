use iced::futures::channel::mpsc;
use iced::widget::{
    button, center, column, container, row, rule, scrollable, space, table,
    text, text_editor,
};
use iced::{Center, Element, Fill, Shrink, Task, Theme};
use polars_core::prelude::*;

use emporium::data::{Command, Event, Response, ToolResult};
use emporium::{Error, Extension};

pub fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("polygon • iced example")
        .font(include_bytes!("../IBMPlexMono-Regular.ttf"))
        .theme(Theme::CatppuccinMocha)
        .centered()
        .run()
}

struct App {
    extension: Option<emporium::Extension>,
    sender: Option<mpsc::UnboundedSender<Command>>,
    data: ViewData,
    status: String,
}

enum ViewData {
    None,
    Table(DataFrame),
    Text(text_editor::Content),
}

#[derive(Debug, Clone)]
enum Message {
    Loaded(Result<Extension, Error>),
    Event(Event),
    Evented,
    FetchTickers,
    DiscoverTools,
    ListModules,
    GetAggregatesSchema,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            extension: None,
            sender: None,
            data: ViewData::None,
            status: "Loading...".to_string(),
        };

        let extension_path = std::env::var("CARGO_MANIFEST_DIR").unwrap()
            + "/../../marketplace/build/xt-polygon/";
        eprintln!("Extension path: {}", extension_path);

        let config = serde_json::json!({
            "api_key": std::env::var("POLYGON_API_KEY").expect("POLYGON_API_KEY"),
            "base_url": "https://api.polygon.io"
        })
        .to_string();

        let task = Task::perform(
            emporium::load(
                "polygon".to_string(),
                config,
                extension_path.into(),
            ),
            Message::Loaded,
        );

        (app, task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(Ok(ext)) => {
                self.status = "Loaded".to_string();

                let sip = ext.clone().into_sipper();
                self.extension = Some(ext);

                return Task::sip(sip, Message::Event, |_| Message::Evented);
            }
            Message::Loaded(Err(e)) => {
                self.status = "Error loading extension".to_string();
                eprintln!("Error loading extension: {e}");
            }
            Message::Event(response) => match response {
                Event::Connected(sender) => {
                    self.sender = Some(sender);
                    return Task::done(Message::FetchTickers);
                }
                Event::Core(core_response) => match core_response {
                    Response::Metadata {
                        id,
                        name,
                        version,
                        description,
                    } => {
                        eprintln!(
                            "Extension metadata: {} {} v{}",
                            id, name, version
                        );
                        eprintln!("  Description: {}", description);
                        self.status = "Ready".to_string();
                    }
                    Response::ToolList {
                        tools,
                        correlation_id,
                    } => {
                        self.status = format!(
                            "Found {} tools (ref. {})",
                            tools.len(),
                            correlation_id.unwrap_or_default()
                        );

                        let mut text = String::from("Available Tools\n");
                        text.push_str(&"=".repeat(50));
                        text.push('\n');
                        for tool in tools {
                            text.push_str(&format!(
                                "\n• {} ({})\n  {}\n",
                                tool.name, tool.id, tool.description
                            ));
                        }

                        self.data = ViewData::Text(
                            text_editor::Content::with_text(&text),
                        );
                    }
                    Response::ToolDetails {
                        tool_id,
                        tool_info,
                        correlation_id,
                    } => {
                        self.status = format!(
                            "Tool details for '{} ({})'",
                            tool_info.name, tool_id
                        );

                        let mut text =
                            format!("Tool Details: {}\n", tool_info.name);
                        text.push_str(&"=".repeat(50));
                        text.push_str(&format!("\n\nID: {}", tool_info.id));
                        text.push_str(&format!(
                            "\nCorrelation ID: {}",
                            correlation_id.unwrap_or("-".to_string())
                        ));
                        text.push_str(&format!(
                            "\nDescription: {}",
                            tool_info.description
                        ));
                        text.push_str("\n\nSchema:\n");
                        text.push_str(
                            &serde_json::to_string_pretty(&tool_info.schema)
                                .unwrap_or_default(),
                        );

                        self.data = ViewData::Text(
                            text_editor::Content::with_text(&text),
                        );
                    }
                    Response::ToolResult {
                        tool_id,
                        result,
                        correlation_id,
                    } => match result {
                        Ok(tool_result) => {
                            self.status = format!(
                                "Result from '{}' (ref. {})",
                                tool_id,
                                correlation_id.unwrap_or("-".to_string())
                            );

                            match tool_result {
                                ToolResult::Text(text) => {
                                    self.data = ViewData::Text(
                                        text_editor::Content::with_text(&text),
                                    );
                                }
                                ToolResult::DataFrame(df) => {
                                    self.data = ViewData::Table(df);
                                }
                            }
                        }
                        Err(e) => {
                            self.status =
                                format!("Error from '{}': {}", tool_id, e);
                        }
                    },
                    Response::Error {
                        message,
                        correlation_id,
                    } => {
                        self.status = format!(
                            "Error: {} (ref. {})",
                            message,
                            correlation_id.unwrap_or_default()
                        );
                    }
                },
            },
            Message::FetchTickers => {
                let Some(sender) = &self.sender else {
                    self.status = "No sender available yet".to_string();
                    return Task::none();
                };

                // Use the ExecuteTool command variant
                let command = Command::execute_tool(
                    "call_endpoint".to_string(),
                    serde_json::json!({
                        "module": "Tickers",
                        "endpoint": "all",
                        "arguments": {
                            "limit": 100
                        }
                    }),
                    None,
                );

                self.status = "Fetching tickers...".to_string();

                if let Err(e) = sender.unbounded_send(command) {
                    self.status = format!("Error sending message: {:?}", e);
                }
            }
            Message::Evented => {
                self.status = "Ready".to_string();
            }
            Message::DiscoverTools => {
                let Some(sender) = &self.sender else {
                    self.status = "No sender available yet".to_string();
                    return Task::none();
                };

                let command = Command::list_tools(None);

                self.status = "Discovering tools...".to_string();

                if let Err(e) = sender.unbounded_send(command) {
                    self.status = format!("Error sending message: {:?}", e);
                }
            }
            Message::ListModules => {
                let Some(sender) = &self.sender else {
                    self.status = "No sender available yet".to_string();
                    return Task::none();
                };

                let command = Command::execute_tool(
                    "list_modules".to_string(),
                    serde_json::json!({}),
                    None,
                );

                self.status = "Listing modules...".to_string();

                if let Err(e) = sender.unbounded_send(command) {
                    self.status = format!("Error sending message: {:?}", e);
                }
            }
            Message::GetAggregatesSchema => {
                let Some(sender) = &self.sender else {
                    self.status = "No sender available yet".to_string();
                    return Task::none();
                };

                let command = Command::execute_tool(
                    "get_endpoint_schema".to_string(),
                    serde_json::json!({
                        "module": "Aggs",
                        "endpoint": "GroupedDaily"
                    }),
                    None,
                );

                self.status = "Getting aggregates schema...".to_string();

                if let Err(e) = sender.unbounded_send(command) {
                    self.status = format!("Error sending message: {:?}", e);
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let title = container(text("polygon.io").size(13).style(text::primary))
            .padding(10);

        let controls = container(
            column![
                text("Discovery:").size(12).style(text::secondary),
                row![
                    button("List Tools").on_press(Message::DiscoverTools),
                    button("List Modules").on_press(Message::ListModules),
                    button("Aggs Schema")
                        .on_press(Message::GetAggregatesSchema),
                ]
                .spacing(10),
                space().height(10),
                text("Data:").size(12).style(text::secondary),
                row![button("Fetch Tickers").on_press(Message::FetchTickers),]
                    .spacing(10)
            ]
            .spacing(5),
        )
        .padding(10);

        let body = match &self.data {
            ViewData::None => Element::from(space().height(Fill)),

            ViewData::Table(data) => {
                center(scrollable(dataframe(data)).spacing(10).direction(
                    scrollable::Direction::Both {
                        vertical: scrollable::Scrollbar::default(),
                        horizontal: scrollable::Scrollbar::default(),
                    },
                ))
                .padding(10)
                .into()
            }

            ViewData::Text(content) => container(scrollable(
                text_editor(content)
                    .placeholder("No content")
                    .font(iced::Font::with_name("IBM Plex Mono"))
                    .size(12),
            ))
            .padding(10)
            .into(),
        };

        let footer = column![
            rule::horizontal(1).style(rule::weak),
            container(
                text(self.status.to_uppercase())
                    .size(11)
                    .style(text::secondary)
            )
            .padding([5, 10])
        ];

        column![title, controls, body, footer].into()
    }
}

fn dataframe<'a>(df: &'a DataFrame) -> Element<'a, Message> {
    let table_columns = df.get_columns().iter().map(|col| {
        let col_name = col.name().to_uppercase();
        let series = col.as_materialized_series();
        table::column(header(col_name), move |i| cell(&series, i))
            .align_x(Center)
            .align_y(Center)
    });

    container(
        column![
            container(title("/tickers/all")).padding([5, 10]),
            rule::horizontal(1).style(rule::weak),
            table(table_columns, 0..df.height())
                .padding_x(10)
                .padding_y(5)
                .separator_x(1)
                .separator_y(1),
        ]
        .width(Shrink),
    )
    .style(container::bordered_box)
    .into()
}

/// Format a cell value from a DataFrame column at a specific row index
fn cell<'a>(series: &'a Series, i: usize) -> Option<Element<'a, Message>> {
    match series.dtype() {
        DataType::Boolean => {
            series.bool().ok().and_then(|ca| ca.get(i)).map(active)
        }
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64 => series
            .i64()
            .ok()
            .and_then(|ca| ca.get(i))
            .map(|v| fragment(format!("{v}"))),
        DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => series
            .u64()
            .ok()
            .and_then(|ca| ca.get(i))
            .map(|v| fragment(format!("{v}"))),
        DataType::Float32 | DataType::Float64 => series
            .f64()
            .ok()
            .and_then(|ca| ca.get(i))
            .map(|v| fragment(format!("{:.2}", v))),
        DataType::String => {
            series.str().ok().and_then(|ca| ca.get(i)).map(fragment)
        }
        _ => series.get(i).ok().map(|v| fragment(v.to_string())),
    }
}

fn title<'a>(label: &'a str) -> Element<'a, Message> {
    text(label.to_uppercase())
        .font(iced::Font::MONOSPACE)
        .size(11)
        .style(text::primary)
        .into()
}

fn header<'a>(label: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(label).size(12).style(text::secondary).into()
}

fn active<'a>(value: bool) -> Element<'a, Message> {
    if value {
        text("✓").style(text::success).into()
    } else {
        text("✗").style(text::secondary).into()
    }
}

fn fragment<'a>(value: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(value).size(12).into()
}
