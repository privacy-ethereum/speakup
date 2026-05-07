project = "SpeakUp"
copyright = "2026, SpeakUp Contributors"
author = "SpeakUp Contributors"

extensions = [
    "myst_parser",
    "sphinx.ext.mathjax",
    "sphinx.ext.githubpages",
    "sphinx.ext.todo",
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
html_logo = "_static/logo.svg"
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
    ],
}

html_copy_source = False
html_show_sourcelink = False
html_show_sphinx = False

mathjax_path = "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js"

todo_include_todos = True
