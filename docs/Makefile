SPHINXOPTS    ?=
SPHINXBUILD   ?= uv run sphinx-build
SOURCEDIR     = .
BUILDDIR      = _build
DIAGRAMDIR    = ../diagrams
SVGDIR        = _static/diagrams

DRAWIO_FILES  = $(wildcard $(DIAGRAMDIR)/*.drawio)
SVG_FILES     = $(patsubst $(DIAGRAMDIR)/%.drawio,$(SVGDIR)/%.svg,$(DRAWIO_FILES))

help:
	@$(SPHINXBUILD) -M help "$(SOURCEDIR)" "$(BUILDDIR)" $(SPHINXOPTS)

$(SVGDIR)/%.svg: $(DIAGRAMDIR)/%.drawio | $(SVGDIR)
	drawio --export --format svg --no-sandbox --embed-svg-images --svg-theme light --output $@ $<

$(SVGDIR):
	mkdir -p $(SVGDIR)

diagrams: $(SVG_FILES)

html: diagrams
	@$(SPHINXBUILD) -M html "$(SOURCEDIR)" "$(BUILDDIR)" $(SPHINXOPTS)

serve: diagrams
	uv run sphinx-autobuild "$(SOURCEDIR)" "$(BUILDDIR)/html" --watch "$(DIAGRAMDIR)" --pre-build "make -C . diagrams" --port 8081

.PHONY: help diagrams html serve
