// Small on purpose: plain JavaScript, no framework. Page text is built with textContent, never innerHTML.
(function () {
  'use strict';
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };

  function api(path, body) {
    return fetch(path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body)
    }).then(function (r) {
      return r.json().catch(function () { return {}; }).then(function (j) {
        if (!r.ok) { throw new Error(j.error || 'Something went wrong.'); }
        return j;
      });
    });
  }

  // ---- toast (with a 10 second Undo) ----
  var toastTimer;
  function toast(msg, undoId) {
    var t = $('#toast');
    t.textContent = '';
    var span = document.createElement('span');
    span.textContent = msg;
    t.appendChild(span);
    if (undoId) {
      var b = document.createElement('button');
      b.type = 'button';
      b.textContent = 'Undo';
      b.addEventListener('click', function () {
        api('/api/undo', { id: undoId }).then(function () { reloadWith('Undone.'); }).catch(function (e) { toast(e.message); });
      });
      t.appendChild(b);
    }
    t.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { t.hidden = true; }, undoId ? 10000 : 5000);
  }
  function reloadWith(msg, undoId) {
    sessionStorage.setItem('toast', JSON.stringify({ msg: msg, undo: undoId || 0 }));
    location.reload();
  }
  var saved = sessionStorage.getItem('toast');
  if (saved) {
    sessionStorage.removeItem('toast');
    try { var s = JSON.parse(saved); toast(s.msg, s.undo); } catch (e) { /* ignore */ }
  }
  function fail(e) { toast(e.message || 'Something went wrong.'); }

  // Handle the common answers of the change endpoints.
  function handle(res) {
    if (res.status === 'proposal') {
      sessionStorage.setItem('toast', JSON.stringify({ msg: res.message, undo: 0 }));
      location.href = '/plan';
    } else if (res.status === 'none') {
      toast(res.message);
    } else {
      reloadWith(res.message, res.undo);
    }
  }

  // ---- theme ----
  var themeBtn = $('#theme');
  function paintTheme() {
    themeBtn.textContent = document.documentElement.dataset.theme === 'dark' ? 'Light mode' : 'Dark mode';
  }
  themeBtn.addEventListener('click', function () {
    var next = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = next;
    localStorage.setItem('theme', next);
    paintTheme();
  });
  paintTheme();

  // ---- add by sentence ----
  var text = $('#text');
  var preview = $('#preview');
  var chips = $('#chips');
  function readIt() {
    var v = text.value.trim();
    if (!v) { toast('Type a sentence first, for example: Remind me to call John tomorrow at 5 PM'); return; }
    api('/api/parse', { text: v }).then(function (p) {
      var f = chips.elements;
      f.title.value = p.title || '';
      f.date.value = p.date || '';
      f.time.value = p.time || '';
      f.priority.value = String(p.priority);
      f.reminder_min.value = p.reminder_min === null ? '' : String(p.reminder_min);
      f.repeat.value = p.repeat || '';
      f.until.value = p.until || '';
      f.dur_min.value = p.dur_min;
      f.kind.value = p.kind;
      var n = $('#notes');
      n.textContent = '';
      (p.notes || []).forEach(function (x) { var li = document.createElement('li'); li.textContent = x; n.appendChild(li); });
      if (!p.date) {
        var li = document.createElement('li');
        li.textContent = 'No date found. It will go in "No date yet". Pick a date above if you want one.';
        n.appendChild(li);
      }
      preview.hidden = false;
      f.title.focus();
    }).catch(fail);
  }
  function val(x) { return x === '' ? null : x; }
  if (text) {
    $('#read').addEventListener('click', readIt);
    text.addEventListener('keydown', function (e) { if (e.key === 'Enter') { e.preventDefault(); readIt(); } });
    $$('.example').forEach(function (b) {
      b.addEventListener('click', function () { text.value = b.dataset.text; readIt(); });
    });
    $('#discard').addEventListener('click', function () { preview.hidden = true; text.focus(); });
    $('#save').addEventListener('click', function () {
      var f = chips.elements;
      api('/api/tasks', {
        title: f.title.value,
        date: val(f.date.value),
        time: val(f.time.value),
        priority: parseInt(f.priority.value, 10),
        reminder_min: f.reminder_min.value === '' ? null : parseInt(f.reminder_min.value, 10),
        repeat: val(f.repeat.value),
        until: val(f.until.value),
        dur_min: parseInt(f.dur_min.value, 10) || 30,
        kind: f.kind.value
      }).then(function (r) { reloadWith('Saved.', r.undo); }).catch(fail);
    });
  }
  var sample = $('#sample');
  if (sample) { sample.addEventListener('click', function () { api('/api/sample', {}).then(handle).catch(fail); }); }

  // ---- buttons on tasks (works for rows added by the server) ----
  document.addEventListener('click', function (e) {
    var b = e.target.closest('button');
    if (!b) { return; }
    if (b.classList.contains('done')) {
      api('/api/tasks/' + b.dataset.id + '/done', {}).then(handle).catch(fail);
    } else if (b.classList.contains('del')) {
      api('/api/delete', { ids: [parseInt(b.dataset.id, 10)] }).then(handle).catch(fail);
    } else if (b.classList.contains('move')) {
      openMove(b.dataset.id, b.dataset.title, b.dataset.at);
    } else if (b.classList.contains('approve')) {
      api('/api/proposals/' + b.dataset.pid + '/approve', {}).then(handle).catch(fail);
    } else if (b.classList.contains('cancel')) {
      api('/api/proposals/' + b.dataset.pid + '/cancel', {}).then(function (r) { reloadWith(r.message); }).catch(fail);
    } else if (b.classList.contains('undo')) {
      api('/api/undo', { id: parseInt(b.dataset.uid, 10) }).then(function () { reloadWith('Undone.'); }).catch(fail);
    }
  });

  // ---- planner ----
  function plan(missed) { api('/api/plan', { missed: missed }).then(handle).catch(fail); }
  var pd = $('#plan-day'); if (pd) { pd.addEventListener('click', function () { plan(false); }); }
  var pm = $('#plan-missed'); if (pm) { pm.addEventListener('click', function () { plan(true); }); }

  // ---- "Move to..." dialog (phones and keyboard) ----
  var dlg = $('#move-dialog');
  var moving = null;
  function pad(n) { return (n < 10 ? '0' : '') + n; }
  function openMove(id, title, at) {
    moving = parseInt(id, 10);
    $('#move-title').textContent = title || '';
    if (!at) {
      var d = new Date(Date.now() + 3600000);
      at = d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate()) + 'T' + pad(d.getHours()) + ':00';
    }
    $('#move-when').value = at;
    if (dlg.showModal) { dlg.showModal(); } else { dlg.setAttribute('open', ''); }
    $('#move-when').focus();
  }
  function closeMove() { if (dlg.close) { dlg.close(); } else { dlg.removeAttribute('open'); } }
  function sendMove(id, to) {
    api('/api/move', { moves: [{ id: id, to: to }] }).then(handle).catch(fail);
  }
  $('#move-ok').addEventListener('click', function () {
    var v = $('#move-when').value;
    if (!v) { toast('Pick a date and time.'); return; }
    closeMove();
    sendMove(moving, v);
  });
  $('#move-cancel').addEventListener('click', closeMove);

  // ---- drag onto the calendar (mouse; phones use Move to...) ----
  document.addEventListener('dragstart', function (e) {
    var el = e.target.closest && e.target.closest('[data-id][draggable=true]');
    if (!el) { return; }
    e.dataTransfer.setData('text/plain', el.dataset.id);
    e.dataTransfer.effectAllowed = 'move';
  });
  $$('.slot').forEach(function (slot) {
    slot.addEventListener('dragover', function (e) { e.preventDefault(); slot.classList.add('over'); });
    slot.addEventListener('dragleave', function () { slot.classList.remove('over'); });
    slot.addEventListener('drop', function (e) {
      e.preventDefault();
      slot.classList.remove('over');
      var id = parseInt(e.dataTransfer.getData('text/plain'), 10);
      if (id) { sendMove(id, slot.dataset.slot); }
    });
  });

  // ---- reminders ----
  var shown = {};
  function paintBanner(list) {
    var b = $('#banner');
    b.textContent = '';
    if (!list.length) { return; }
    var p = document.createElement('div');
    p.textContent = list.length === 1 ? '1 reminder:' : list.length + ' reminders:';
    var ul = document.createElement('ul');
    list.forEach(function (d) {
      var li = document.createElement('li');
      li.textContent = d.title + (d.when ? ' (' + d.when + ')' : '');
      ul.appendChild(li);
    });
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.textContent = 'Dismiss';
    btn.addEventListener('click', function () {
      api('/api/due/ack', { ids: list.map(function (d) { return d.id; }) }).then(poll).catch(fail);
    });
    b.appendChild(p); b.appendChild(ul); b.appendChild(btn);
  }
  function poll() {
    api('/api/due').then(function (r) {
      paintBanner(r.due);
      r.due.forEach(function (d) {
        if (!shown[d.id] && 'Notification' in window && Notification.permission === 'granted') {
          shown[d.id] = true;
          new Notification('Reminder: ' + d.title, { body: d.when });
        }
      });
    }).catch(function () { /* server may be restarting */ });
  }
  var nb = $('#notify');
  if (nb) {
    nb.addEventListener('click', function () {
      if (!('Notification' in window)) { toast('This browser cannot show pop-ups.'); return; }
      Notification.requestPermission().then(function (p) {
        toast(p === 'granted' ? 'Pop-up reminders are on while this page is open.' : 'Pop-ups are off. Reminders still show in the yellow bar.');
      });
    });
  }
  poll();
  setInterval(poll, 30000);
})();
