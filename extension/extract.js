// Injected after Readability.js; its value is the last expression below.
(() => {
  try {
    const article = new Readability(document.cloneNode(true)).parse();
    const text = (article && article.textContent) || document.body.innerText || '';
    return text.replace(/\s+/g, ' ').trim().slice(0, 2000);
  } catch (e) {
    return (document.body.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 2000);
  }
})();
