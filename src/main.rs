use std::fs;
use std::path::{Path, PathBuf};
use async_channel::{Receiver, Sender};
use futures::future::join_all;
use html_escape::decode_html_entities;
use reqwest::Client;
use sanitize_filename_reader_friendly::sanitize;
use strum::IntoEnumIterator;
use strum_macros::{Display, EnumIter};
use tl::{HTMLTag, Parser};
use tokio::io::AsyncWriteExt;

#[derive(Display, EnumIter)]
enum SheetFormat {
    PDF,
    MID,
    MUS
}

#[derive(Debug)]
struct Serie {
    name: String,
    url: String,
    games: Vec<Game>
}

#[derive(Debug)]
struct Game {
    name: String,
    system: String,
    sheets: Vec<Sheet>
}

#[derive(Debug, Clone)]
struct Sheet {
    name: String,
    arrangers: Vec<String>,
    id: i32
}

struct QueuedDownload {
    path: PathBuf,
    sheet: Sheet
}

impl Serie {
    async fn populate_games(&mut self, client: &Client) -> Result<(), reqwest::Error> {
        let response = client.get(&self.url).send().await?.text().await?;

        let dom = tl::parse(&response, tl::ParserOptions::default()).unwrap();
        let parser = dom.parser();

        let game_sections = dom.get_elements_by_class_name("game");
        let games: Vec<Game> = game_sections.map(|node_hdl| Game::parse(node_hdl.get(parser).unwrap().as_tag().unwrap(), parser)).collect();

        self.games = games;

        Ok(())
    }

    fn parse(a_tag: &HTMLTag, parser: &Parser) -> Serie {
        let href = a_tag.attributes().get("href").flatten().unwrap().as_utf8_str();
        let name = a_tag.inner_text(parser);

        Serie {
            name: String::from(decode_html_entities(&name)),
            url: format!("https://www.ninsheetmusic.org{}", href),
            games: Vec::new()
        }
    }
}

impl Game {
    fn parse(section: &HTMLTag, parser: &Parser) -> Game {
        let heading_text = section.query_selector(parser, "h3").unwrap().next().unwrap();
        let name = heading_text.get(parser).unwrap().inner_text(parser);
        let console_a = section.query_selector(parser, "a[title]").unwrap().next().unwrap();
        let system = console_a.get(parser).unwrap().as_tag().unwrap().attributes().get("title").flatten().unwrap().as_utf8_str();

        let sheets: Vec<Sheet> = section.query_selector(parser, "li.tableList-row--sheet").unwrap()
            .map(|node_hdl| Sheet::parse(node_hdl.get(parser).unwrap().as_tag().unwrap(), parser)).collect();

        Game {
            name: String::from(decode_html_entities(&name)),
            system: String::from(decode_html_entities(&system)),
            sheets
        }
    }
}

impl Sheet {
    fn parse(li: &HTMLTag, parser: &Parser) -> Sheet {
        let id: i32 = li.attributes().id().unwrap().as_utf8_str()[5..].parse().unwrap();
        let title_element = li.query_selector(parser, "div.tableList-cell--sheetTitle").unwrap().next().unwrap();
        let name = title_element.get(parser).unwrap().inner_text(parser);
        let arranger_element = li.query_selector(parser, "div.tableList-cell--sheetArranger").unwrap().next().unwrap();
        let arrangers: Vec<String> = arranger_element.get(parser).unwrap().as_tag().unwrap().query_selector(parser, "a[href]").unwrap()
            .map(|node_hdl| String::from(decode_html_entities(&node_hdl.get(parser).unwrap().inner_text(parser)))).collect();

        Sheet {
            name: String::from(decode_html_entities(&name)),
            arrangers,
            id
        }
    }

    fn get_download_url(&self, format: SheetFormat) -> String {
        format!("https://www.ninsheetmusic.org/download/{}/{}", format.to_string().to_lowercase(), self.id)
    }

    async fn download(&self, folder_path: &Path, format: SheetFormat, client: &Client) -> Result<(), reqwest::Error> {
        let path = folder_path.join(format!("{}.{}", sanitize(&self.name), format.to_string().to_lowercase()));
        let response = client.get(self.get_download_url(format)).send().await?.bytes().await?;

        let mut file = tokio::fs::File::create(path).await.expect("Couldn't create file.");
        file.write_all(&response[..]).await.expect("Couldn't write to file.");

        Ok(())
    }
}

async fn fetch_series(client: &Client) -> Result<Vec<Serie>, reqwest::Error> {
    let response = client.get("https://www.ninsheetmusic.org/browse/series").send().await?.text().await?;

    let dom = tl::parse(&response, tl::ParserOptions::default()).unwrap();
    let parser = dom.parser();

    let link_tags = dom.nodes()
        .iter()
        .filter(|node| {
           node.as_tag().map_or(false, |tag| {
               let attributes = tag.attributes();
               tag.name() == "a" && attributes.get("href").flatten()
                   .map_or(false, |bytes| bytes.as_utf8_str().starts_with("/browse/series/") && !bytes.as_utf8_str().contains('#'))
           })
        });

    let series: Vec<Serie> = link_tags.map(|node| Serie::parse(node.as_tag().unwrap(), parser)).collect();

    Ok(series)
}

const THREAD_COUNT: i32 = 6;

#[tokio::main]
async fn main() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    println!("nsm_archiver v{} by Linkster78\n", VERSION);

    let client = Client::new();

    println!("> Indexing series...");
    let mut series = fetch_series(&client).await.expect("Failed to pull the NinSheetMusic website for series.");
    println!("< Indexed {} series!", series.len());

    let (tx, rx): (Sender<QueuedDownload>, Receiver<QueuedDownload>) = async_channel::unbounded();

    for serie in series.iter_mut() {
        println!("> Indexing games for serie {}...", serie.name);
        serie.populate_games(&client).await.expect("Failed to pull the NinSheetMusic website for games.");
        println!("< Indexed {} games totalling {} sheets.", serie.games.len(), serie.games.iter().map(|game| game.sheets.len()).sum::<usize>());

        for game in &serie.games {
            let folder_path_str = format!("./downloads/{}/{}/", sanitize(&serie.name), sanitize(&game.name));
            let folder_path = Path::new(&folder_path_str);
            fs::create_dir_all(folder_path).expect("Couldn't create the folder hierarchy.");

            for sheet in &game.sheets {
                let download = QueuedDownload {
                    path: folder_path.to_owned(),
                    sheet: sheet.clone()
                };
                let _ = tx.send(download).await;
            }
        }
    }

    let mut tasks = vec!();

    for _ in 0..THREAD_COUNT {
        let rx = rx.clone();
        let task = tokio::spawn(async move {
            let client = Client::new();

            while let Ok(queued_dl) = rx.recv().await {
                for format in SheetFormat::iter() {
                    queued_dl.sheet.download(&queued_dl.path, format, &client).await?;
                }
                println!("+ Downloaded {} in all formats.", queued_dl.sheet.name);

                if rx.is_empty() {
                    break;
                }
            }

            Ok::<_, reqwest::Error>(())
        });
        tasks.push(task);
    }

    join_all(tasks).await;
}