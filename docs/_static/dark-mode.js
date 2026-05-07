(function () {
    var STORAGE_KEY = "theme";

    function getStoredTheme() {
        try {
            return localStorage.getItem(STORAGE_KEY);
        } catch (e) {
            return null;
        }
    }

    function storeTheme(theme) {
        try {
            localStorage.setItem(STORAGE_KEY, theme);
        } catch (e) {
            /* ignore */
        }
    }

    function resolveInitialTheme() {
        var stored = getStoredTheme();
        if (stored === "dark" || stored === "light") {
            return stored;
        }
        if (
            window.matchMedia &&
            window.matchMedia("(prefers-color-scheme: dark)").matches
        ) {
            return "dark";
        }
        return "light";
    }

    function applyTheme(theme) {
        document.documentElement.dataset.theme = theme;
        var btn = document.getElementById("dark-mode-toggle");
        if (btn) {
            var label = theme === "dark"
                ? "Switch to light mode"
                : "Switch to dark mode";
            btn.setAttribute("aria-label", label);
            btn.setAttribute("title", label);
            btn.setAttribute(
                "aria-pressed",
                theme === "dark" ? "true" : "false"
            );
        }
    }

    // Apply as early as possible to reduce flash.
    applyTheme(resolveInitialTheme());

    function injectToggle() {
        if (document.getElementById("dark-mode-toggle")) return;
        var sidebar = document.querySelector("div.sphinxsidebarwrapper");
        if (!sidebar) return;

        var wrapper = document.createElement("div");
        wrapper.id = "dark-mode-toggle-wrapper";

        var btn = document.createElement("button");
        btn.id = "dark-mode-toggle";
        btn.type = "button";
        btn.addEventListener("click", function () {
            var current = document.documentElement.dataset.theme === "dark"
                ? "dark"
                : "light";
            var next = current === "dark" ? "light" : "dark";
            storeTheme(next);
            applyTheme(next);
        });
        wrapper.appendChild(btn);

        var searchbox = sidebar.querySelector("#searchbox");
        if (searchbox && searchbox.parentNode === sidebar) {
            sidebar.insertBefore(wrapper, searchbox.nextSibling);
        } else {
            sidebar.appendChild(wrapper);
        }

        applyTheme(document.documentElement.dataset.theme || "light");
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", injectToggle);
    } else {
        injectToggle();
    }
})();
