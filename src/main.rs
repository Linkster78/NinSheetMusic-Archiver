use reqwest::blocking::Client;
use strum_macros::Display;
use tl::{HTMLTag, Parser};

#[derive(Display)]
enum SheetFormat {
    PDF,
    MID,
    MUS
}

#[derive(Debug)]
struct Serie {
    name: String,
    url: String
}

#[derive(Debug)]
struct Game {
    name: String,
    system: String,
    sheets: Vec<Sheet>
}

#[derive(Debug)]
struct Sheet {
    name: String,
    arrangers: Vec<String>,
    id: i32
}

impl Serie {
    fn parse(a_tag: &HTMLTag, parser: &Parser) -> Serie {
        let href = a_tag.attributes().get("href").flatten().unwrap().as_utf8_str();
        let name = a_tag.inner_text(parser);

        Serie {
            name: String::from(name),
            url: format!("https://www.ninsheetmusic.org{}", href)
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
            name: String::from(name),
            system: String::from(system),
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
            .map(|node_hdl| String::from(node_hdl.get(parser).unwrap().inner_text(parser))).collect();

        Sheet {
            name: String::from(name),
            arrangers,
            id
        }
    }

    fn download_url(&self, format: SheetFormat) -> String {
        format!("https://www.ninsheetmusic.org/download/{}/{}", format.to_string().to_lowercase(), self.id)
    }
}

fn fetch_series(client: &Client) -> Result<Vec<Serie>, reqwest::Error> {
    let response = client.get("https://www.ninsheetmusic.org/browse/series").send()?.text()?;

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

fn fetch_games(client: &Client, url: &str) -> Result<Vec<Game>, reqwest::Error> {
    let response = client.get(url).send()?.text()?;

    let dom = tl::parse(&response, tl::ParserOptions::default()).unwrap();
    let parser = dom.parser();

    let game_sections = dom.get_elements_by_class_name("game");
    let games: Vec<Game> = game_sections.map(|node_hdl| Game::parse(node_hdl.get(parser).unwrap().as_tag().unwrap(), parser)).collect();

    Ok(games)
}

fn main() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    println!("nsm_archiver v{} by Linkster78\n", VERSION);

    let client = Client::new();

    println!("> Fetching series list...");
    let series = fetch_series(&client).expect("Failed to pull the NinSheetMusic website for series.");
    println!("< Fetched {} series!", series.len());

    for serie in series {
        println!("> Fetching games for {}...", serie.name);
        let games = fetch_games(&client, &serie.url).expect("Failed to pull the NinSheetMusic website for games.");
        println!("< Fetched {} games totalling {} sheets.", games.len(), games.iter().map(|game| game.sheets.len()).sum::<usize>());
    }
}