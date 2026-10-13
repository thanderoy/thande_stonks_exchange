// Applies a saved theme choice before first paint so the page does not flash.
// Loaded as a blocking same-origin script, so the CSP needs no inline hash.
(function () {
  try {
    var t = localStorage.getItem("tse-theme");
    if (t === "light" || t === "dark") {
      document.documentElement.setAttribute("data-theme", t);
    }
  } catch (e) {
    // Storage blocked (private mode, disabled site data): follow the system.
  }
})();
