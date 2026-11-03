// Imported first by each page, so Object.prototype is frozen before any other module runs. Tauri's
// freezePrototype setting would do the same, but it reaches every window, and the username
// checker's hidden window loads sites whose scripts break under it.
Object.freeze(Object.prototype);
