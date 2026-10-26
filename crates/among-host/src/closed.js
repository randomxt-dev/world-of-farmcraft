// Among Azeroth: among-host puts this before the game page's own scripts. The lobby gate answers
// 403 once the page's lobby has closed, which the client would otherwise sit on as a black
// screen, so any 403 from this origin shows a notice with the way back to the landing page.
(() => {
  let shown = false;
  const show = () => {
    if (shown) return;
    shown = true;
    const put = () => {
      const box = document.createElement('div');
      box.id = 'among-closed';
      box.setAttribute('role', 'alert');
      box.style.cssText =
        'position:fixed;inset:0;z-index:2147483647;display:flex;align-items:center;' +
        'justify-content:center;background:rgba(0,0,0,.85);color:#eee;' +
        'font:18px system-ui,sans-serif;text-align:center;padding:1rem';
      box.innerHTML =
        '<p>This lobby has closed. Start or join one at <a href="/" style="color:#ffd100">' +
        location.host + '/</a></p>';
      document.body.appendChild(box);
    };
    document.body ? put() : addEventListener('DOMContentLoaded', put);
  };
  const ours = (url) => {
    try {
      return new URL(url, location.href).origin === location.origin;
    } catch {
      return false;
    }
  };
  const fetch0 = window.fetch;
  window.fetch = function (input, init) {
    const url = input instanceof Request ? input.url : String(input);
    return fetch0.call(this, input, init).then((r) => {
      if (r.status === 403 && ours(url)) show();
      return r;
    });
  };
  // The client reads game files with synchronous XHR, which has finished when `send` returns.
  const open0 = XMLHttpRequest.prototype.open;
  const send0 = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    this.__amongOurs = ours(url);
    return open0.call(this, method, url, ...rest);
  };
  XMLHttpRequest.prototype.send = function (...args) {
    const check = () => {
      if (this.readyState === 4 && this.status === 403 && this.__amongOurs) show();
    };
    this.addEventListener('loadend', check);
    try {
      return send0.apply(this, args);
    } finally {
      check();
    }
  };
  // A refused WebSocket hides its status, so a socket that closes asks a gated file instead.
  const probe = () =>
    window.fetch('./wasi_stubs.js', { method: 'HEAD', cache: 'no-store' }).catch(() => {});
  window.WebSocket = new Proxy(window.WebSocket, {
    construct(target, args, newTarget) {
      const socket = Reflect.construct(target, args, newTarget);
      socket.addEventListener('close', probe);
      return socket;
    },
  });
})();
