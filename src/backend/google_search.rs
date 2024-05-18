async fn request(
    client: &reqwest::Client,
    request: reqwest::Request,
) -> Result<bytes::BytesMut, reqwest::Error> {
    dbg!(request.url().as_str());
    let mut response = client.execute(dbg!(request)).await?.error_for_status()?;
    let mut buffer = bytes::BytesMut::new();
    loop {
        // TODO: Is response.chunk() cancellation safe?
        if let Ok(result) =
            tokio::time::timeout(std::time::Duration::new(0, 0), response.chunk()).await
        {
            if let Some(chunk) = result? {
                bytes::BufMut::put(&mut buffer, chunk);
            } else {
                break;
            }
        }
    }
    Ok(buffer)
}

async fn google_search(
    client: &reqwest::Client,
    q: &str,
) -> Result<bytes::BytesMut, reqwest::Error> {
    Ok(request(
        &client,
        client
            .get("http://www.google.com/search")
            // make google return UTF-8
            .header("User-Agent", "Mozilla/5.0")
            .query(&[("hl", "en"), ("q", q)])
            .build()
            .unwrap(),
    )
    .await?)
}

fn parse_google_search_texts(document: scraper::Html) -> Vec<String> {
    let div_selector = scraper::Selector::parse(
        "#main > div > div > div:nth-child(2) > div > div > div > div:nth-child(1)",
    )
    .unwrap();
    document
        .select(&div_selector)
        .filter_map(|div| {
            if div.attr("role") == None {
                Some(dbg!(div.text().collect::<Vec<_>>().join(" ")))
            } else {
                None
            }
        })
        .collect()
}

fn parse_google_search_urls(document: scraper::Html) -> Vec<String> {
    let div_selector = scraper::Selector::parse("#main > div").unwrap();
    let anchor_selector = scraper::Selector::parse(":scope > div > div > a").unwrap();
    let mut urls = vec![];
    for div in document.select(&div_selector) {
        for anchor in div.select(&anchor_selector) {
            let Some(href) = anchor.attr("href") else {
                continue;
            };
            let domain = uuid::Uuid::new_v4().to_string();
            let url = url::Url::parse(&format!("http://{}/", domain))
                .unwrap()
                .join(href)
                .unwrap();
            if url.domain() != Some(&domain) {
                continue;
            }
            if url.path() != "/url" {
                continue;
            }
            let Some(q) = url
                .query_pairs()
                .find_map(|(name, value)| if name == "q" { Some(value) } else { None })
            else {
                continue;
            };
            urls.push(q.to_string());
            break;
        }
    }
    urls
}

async fn get_bodies<T>(client: &reqwest::Client, urls: T) -> Vec<bytes::BytesMut>
where
    T: std::iter::IntoIterator,
    T::Item: reqwest::IntoUrl,
{
    let mut bodies = vec![];
    for url in urls {
        let Ok(body) = request(&client, client.get(url).build().unwrap()).await else {
            continue;
        };
        bodies.push(body);
    }
    bodies
}

pub(crate) async fn retrieve_context(query: &str, simple: bool) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::new();
    let body = google_search(&client, query).await?;
    std::fs::write("index.html", &body).unwrap();
    let document = scraper::Html::parse_document(std::str::from_utf8(&body)?);
    Ok(if simple {
        parse_google_search_texts(document)
    } else {
        get_bodies(
            &client,
            parse_google_search_urls(document)
                .into_iter()
                .filter_map(|url| {
                    if let Ok(mut url) = url::Url::parse(&url) {
                        // hyper-wasi does not support https
                        url.set_scheme("http").ok()?;
                        Some(url)
                    } else {
                        None
                    }
                }),
        )
        .await
        .into_iter()
        //
        .map(|body| String::from_utf8_lossy(&body).into_owned())
        .collect()
    })
}
