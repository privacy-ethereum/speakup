project = "SpeakUp"
copyright = "2026, SpeakUp Contributors"
author = "SpeakUp Contributors"

extensions = [
    "myst_parser",
    "sphinx.ext.mathjax",
    "sphinx.ext.githubpages",
    "sphinx.ext.todo",
    "sphinxext.opengraph",
]

myst_enable_extensions = [
    "dollarmath",
    "amsmath",
    "deflist",
    "colon_fence",
]

templates_path = ["_templates"]
exclude_patterns = ["_build"]

html_theme = "alabaster"
html_title = "SpeakUp"
html_logo = "_static/logo.svg"
html_favicon = "_static/favicon.svg"
html_static_path = ["_static"]
html_js_files = ["dark-mode.js"]
html_theme_options = {
    "sidebar_width": "200px",
    "page_width": "1000px",
    "description": "",
    "fixed_sidebar": True,
    "sidebar_collapse": True,
    "show_powered_by": False,
}

html_sidebars = {
    "**": [
        "navigation.html",
        "searchbox.html",
        "github.html",
    ],
}

html_copy_source = False
html_show_sourcelink = False
html_show_sphinx = False

mathjax_path = "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js"

# Social link previews (Open Graph + Twitter Card). The extension fills
# og:title/og:description per page; ogp_image must be an absolute URL and a
# raster format (SVG is ignored by most scrapers).
ogp_site_url = "https://privacy-ethereum.github.io/speakup/"
ogp_image = "https://privacy-ethereum.github.io/speakup/_static/social-card.png"
ogp_use_first_image = False
ogp_enable_meta_description = True
# Pin the shared card; don't let the extension auto-generate per-page images.
ogp_social_cards = {"enable": False}
# Twitter falls back to OG tags but defaults to a small card without this.
ogp_custom_meta_tags = [
    '<meta name="twitter:card" content="summary_large_image" />',
]

todo_include_todos = True
