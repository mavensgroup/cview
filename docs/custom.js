// src/custom.js
document.addEventListener("DOMContentLoaded", function() {
    // Check if the element with id "cover-page" exists
    if (document.getElementById("cover-page")) {

        // Hide the sidebar
        var sidebar = document.querySelector(".sidebar");
        if (sidebar) sidebar.style.display = "none";

        // Hide the menu bar (hamburger menu)
        var menuBar = document.querySelector(".menu-bar");
        if (menuBar) menuBar.style.display = "none";

        // Force full width for the content
        var content = document.querySelector(".content");
        if (content) {
            content.style.maxWidth = "100%";
            content.style.margin = "0 auto";
        }
    }
});
