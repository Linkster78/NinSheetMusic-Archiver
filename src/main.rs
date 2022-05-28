use reqwest::blocking::Client;
use strum_macros::Display;

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
    author: String,
    id: i32
}

impl Sheet {
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
        }).into_iter();

    let mut serie_list = Vec::new();

    for elem in link_tags {
        let tag = elem.as_tag().unwrap();
        let href = tag.attributes().get("href").flatten().unwrap().as_utf8_str();
        let name = tag.inner_text(parser);

        let serie = Serie {
            name: String::from(name),
            url: format!("https://www.ninsheetmusic.org{}", href)
        };
        serie_list.push(serie);
    }

    Ok(serie_list)
}

fn fetch_games(client: &Client, url: &str) -> Result<Vec<Game>, reqwest::Error> {
    let response = client.get(url).send()?.text()?;

    println!("{}", response);

    Ok(Vec::<Game>::new())
}

fn main() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    println!("nsm_archiver v{} by Linkster78\n", VERSION);

    let client = Client::new();

    println!("> Fetching series list");
    let series = fetch_series(&client).expect("Failed to pull the NinSheetMusic website for series.");
    println!("< Fetched {} series!", series.len());

    for serie in series {
        let games = fetch_games(&client, &serie.url).expect("Failed to pull the NinSheetMusic website for games.");
        println!("{:?}", games);
    }
}