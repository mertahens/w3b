mod common;
use common::fake_app;
use w3b::FakeFetcher;

const U: &str = "https://test.local/x";

#[test]
fn audit() {
    let cases: [(&str, &str); 12] = [
        ("li with p", "<ul><li><p>alpha</p><div>beta</div></li><li>gamma</li></ul>"),
        ("nested list", "<ul><li>one<ul><li>two<ul><li>three</li></ul></li></ul></li></ul>"),
        ("li two blocks", "<ol><li><h2>head</h2><p>body text</p></li></ol>"),
        ("li span+a", "<ul><li><span>a</span><a href=/x>b</a></li></ul>"),
        ("div inline", "<style>div { display: inline }</style><div>a</div><div>b</div>"),
        ("p inline", "<style>p { display: inline }</style><p>a</p><p>b</p>"),
        ("span block", "<style>span { display: block }</style><p>a<span>b</span>c</p>"),
        ("css comment", "<style>.a { /* off */ color: red; display: none }</style><p class=a>gone</p><p>keep</p>"),
        ("style comment head", "<style>/* .a { color: red } */ .b { display: none }</style><p class=b>gone</p><p>keep</p>"),
        ("attr selector", "<style>[hidden] { display: none }</style><p hidden>gone</p><p>keep</p>"),
        ("sibling sel", "<style>h1 + p { display: none }</style><h1>t</h1><p>gone</p><p>keep</p>"),
        ("media min-width", "<style>@media (min-width: 768px) { .nav { display: none } }</style><p class=nav>mobile nav</p><p>keep</p>"),
    ];
    for (name, html) in cases {
        let app = fake_app(FakeFetcher::new().with_html(U, 200, html), U, 40, 20);
        let out: Vec<String> = (0..app.rows().len())
            .map(|r| {
                app.rows()[r].runs.iter().map(|x| x.text.as_str()).collect::<String>().trim_end().to_string()
            })
            .filter(|t| !t.is_empty())
            .collect();
        println!("{name:20} => {out:?}");
    }
}
