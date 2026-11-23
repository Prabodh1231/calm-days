// Small on purpose: plain JavaScript, no framework. Text from cards is set with textContent, never innerHTML.
// Only server-made, already-escaped page HTML is swapped in by refresh().
(function () {
  'use strict';
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  function el(tag, attrs, kids) {
    var n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      if (k === 'text') { n.textContent = attrs[k]; } else if (k === 'class') { n.className = attrs[k]; } else { n.setAttribute(k, attrs[k]); }
    });
    (kids || []).forEach(function (c) { n.appendChild(c); });
    return n;
  }

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

  // ---- toast (10 second Undo) ----
  var toastTimer;
  function toast(msg, undoId) {
    var t = $('#toast');
    t.textContent = '';
    t.appendChild(el('span', { text: msg }));
    if (undoId) {
      var b = el('button', { type: 'button', text: 'Undo' });
      b.addEventListener('click', function () {
        api('/api/undo', { id: undoId }).then(function () { refresh('Undone.'); }).catch(fail);
      });
      t.appendChild(b);
    }
    t.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { t.hidden = true; }, undoId ? 10000 : 5000);
  }
  function fail(e) { toast(e.message || 'Something went wrong.'); }
  var saved = sessionStorage.getItem('toast');
  if (saved) {
    sessionStorage.removeItem('toast');
    try { var sv = JSON.parse(saved); toast(sv.msg, sv.undo); } catch (e) { /* ignore */ }
  }

  // ---- search and filter (browser side) ----
  function applyFilter() {
    var q = $('#q'); if (!q) { return; }
    var text = q.value.trim().toLowerCase(), lab = $('#f-label').value, due = $('#f-due').value;
    var shown = 0, total = 0;
    $$('.bcard').forEach(function (c) {
      total++;
      var ok = (!text || c.dataset.q.indexOf(text) >= 0) &&
        (!lab || (',' + c.dataset.labels + ',').indexOf(',' + lab + ',') >= 0) &&
        (!due || c.dataset.due === due || (due === 'week' && c.dataset.due === 'today'));
      c.hidden = !ok;
      if (ok) { shown++; }
    });
    var active = text || lab || due;
    $('#f-count').textContent = active ? shown + ' of ' + total + ' cards shown' : '';
  }

  // Re-read the page from the server and swap the main area (the panel and toast stay).
  function refresh(msg, undoId) {
    var keep = {};
    ['#q', '#f-label', '#f-due'].forEach(function (s) { var n = $(s); if (n) { keep[s] = n.value; } });
    var b = $('#board'), sl = b ? b.scrollLeft : 0;
    return fetch(location.href).then(function (r) { return r.text(); }).then(function (html) {
      var d = new DOMParser().parseFromString(html, 'text/html');
      $('main').innerHTML = d.querySelector('main').innerHTML;
      Object.keys(keep).forEach(function (s) { var n = $(s); if (n) { n.value = keep[s]; } });
      var nb = $('#board'); if (nb) { nb.scrollLeft = sl; }
      applyFilter();
      if (msg) { toast(msg, undoId); }
    }).catch(fail);
  }
  function handle(res) {
    if (res.status === 'proposal') {
      sessionStorage.setItem('toast', JSON.stringify({ msg: res.message, undo: 0 }));
      location.href = '/plan';
    } else if (res.status === 'none') {
      toast(res.message);
    } else {
      return refresh(res.message, res.undo);
    }
  }

  // ---- theme ----
  var themeBtn = $('#theme');
  function paintTheme() { themeBtn.textContent = document.documentElement.dataset.theme === 'dark' ? 'Light mode' : 'Dark mode'; }
  themeBtn.addEventListener('click', function () {
    var next = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = next;
    localStorage.setItem('theme', next);
    paintTheme();
  });
  paintTheme();

  // ---- board name ----
  $('#board-name').addEventListener('change', function (e) {
    api('/api/board/rename', { name: e.target.value }).then(function (r) { toast(r.message); document.title = document.title.replace(/ - .*$/, ' - ' + e.target.value); }).catch(fail);
  });

  // ---- helpers about lists ----
  function lists() {
    var b = $('#board');
    if (b) { try { return JSON.parse(b.dataset.lists); } catch (e) { /* fall through */ } }
    return [];
  }
  function cardsIn(listId, exceptId) {
    return $$('.cards[data-list="' + listId + '"] .bcard').filter(function (c) { return c.dataset.id !== String(exceptId); });
  }
  function fillLists(sel, selected, skip) {
    sel.textContent = '';
    lists().forEach(function (l) {
      if (String(l.id) === String(skip)) { return; }
      var o = el('option', { value: l.id, text: l.name });
      if (String(l.id) === String(selected)) { o.selected = true; }
      sel.appendChild(o);
    });
  }
  function fillPositions(sel, listId, cardId, selected) {
    var n = cardsIn(listId, cardId).length + 1;
    sel.textContent = '';
    for (var i = 0; i < n; i++) {
      var label = (i + 1) + (i === 0 ? ' (top)' : '') + (i === n - 1 && n > 1 ? ' (bottom)' : '');
      var o = el('option', { value: i, text: label });
      if (i === selected) { o.selected = true; }
      sel.appendChild(o);
    }
  }
  function openDlg(d) { if (d.showModal) { d.showModal(); } else { d.setAttribute('open', ''); } }
  function closeDlg(d) { if (d.close) { d.close(); } else { d.removeAttribute('open'); } }
  function moveCards(moves) { return api('/api/cards/move', { moves: moves }).then(handle).catch(fail); }

  // ---- add a card by sentence ----
  function fmtDate(p) {
    if (!p.date) { return ''; }
    var d = new Date(p.date + 'T' + (p.time || '00:00'));
    var s = d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
    return p.time ? s + ', ' + d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) : s;
  }
  var PRI = ['Low', 'Medium', 'High'];
  function showPreview(form, p) {
    var pv = $('.prev', form);
    pv.textContent = '';
    var add = function (t, cls) { pv.appendChild(el('span', { class: 'chip' + (cls ? ' ' + cls : ''), text: t })); };
    add(p.title || '(no title)');
    if (p.date) { add('Due ' + fmtDate(p)); } else { add('No date found', 'warnchip'); }
    add('Priority: ' + PRI[p.priority]);
    if (p.repeat) { add('Repeats: ' + p.repeat); }
    if (p.reminder_min === -1) { add('No reminder'); } else if (p.date) { add(p.reminder_min === null ? 'Reminder: default' : 'Reminder: ' + p.reminder_min + ' min before'); }
    if (p.kind === 'event') { add('Fixed event'); }
    (p.notes || []).forEach(function (n) { add(n, 'warnchip'); });
    form._parsed = p;
    form._text = $('.add-text', form).value;
    $('.add-save', form).hidden = false;
  }
  function readCard(form) {
    var v = $('.add-text', form).value.trim();
    if (!v) { toast('Type a sentence first, for example: Remind me to call John tomorrow at 5 PM'); return; }
    api('/api/parse', { text: v }).then(function (p) { showPreview(form, p); }).catch(fail);
  }
  function saveCard(form) {
    var p = form._parsed;
    if (!p) { return; }
    api('/api/tasks', {
      title: p.title, date: p.date, time: p.time, priority: p.priority, reminder_min: p.reminder_min,
      repeat: p.repeat || null, until: p.until, dur_min: p.dur_min, kind: p.kind,
      list_id: parseInt($('.add-open', form.parentNode).dataset.list, 10)
    }).then(function (r) { refresh('Card added.', r.undo); }).catch(fail);
  }
  function closeAdder(form) {
    form.hidden = true; form._parsed = null;
    $('.prev', form).textContent = ''; $('.add-text', form).value = ''; $('.add-save', form).hidden = true;
    $('.add-open', form.parentNode).hidden = false;
  }

  // ---- card detail panel ----
  var panel = $('#panel');
  function closePanel() { panel.hidden = true; panel.textContent = ''; }
  var LABELS = ['green', 'yellow', 'orange', 'red', 'purple', 'blue'];
  function field(label, input, id) {
    input.id = id;
    return el('div', {}, [el('label', { for: id, text: label }), input]);
  }
  function option(v, t, sel) { var o = el('option', { value: v, text: t }); if (sel) { o.selected = true; } return o; }
  function openPanel(id) {
    api('/api/cards/' + id).then(function (c) { buildPanel(c); }).catch(fail);
  }
  function buildPanel(c) {
    panel.textContent = '';
    var closeB = el('button', { type: 'button', text: 'Close', 'aria-label': 'Close card details' });
    closeB.addEventListener('click', closePanel);
    panel.appendChild(el('h2', {}, [el('span', { text: c.done ? 'Card (done)' : 'Card' }), closeB]));
    var title = el('input', { type: 'text', maxlength: '200' }); title.value = c.title;
    panel.appendChild(field('Title', title, 'p-title'));
    var notes = el('textarea', { rows: '4', maxlength: '5000', placeholder: 'Add notes...' }); notes.value = c.notes;
    panel.appendChild(field('Notes', notes, 'p-notes'));
    var date = el('input', { type: 'date' }); date.value = c.date || '';
    var time = el('input', { type: 'time' }); time.value = c.time || '';
    panel.appendChild(el('div', { class: 'two' }, [field('Due date', date, 'p-date'), field('Due time', time, 'p-time')]));
    var pri = el('select', {}, [option('0', 'Low', c.priority === 0), option('1', 'Medium', c.priority === 1), option('2', 'High', c.priority === 2)]);
    var rem = el('select', {}, [option('', 'Default', c.reminder_min === null), option('5', '5 min before', c.reminder_min === 5), option('30', '30 min before', c.reminder_min === 30),
      option('60', '1 hour before', c.reminder_min === 60), option('1440', '1 day before', c.reminder_min === 1440), option('-1', 'No reminder', c.reminder_min === -1)]);
    panel.appendChild(el('div', { class: 'two' }, [field('Priority', pri, 'p-pri'), field('Reminder', rem, 'p-rem')]));
    var rep = el('input', { type: 'text', placeholder: 'none, or: every week on mon and wed' }); rep.value = c.repeat || '';
    var until = el('input', { type: 'date' }); until.value = c.until || '';
    panel.appendChild(el('div', { class: 'two' }, [field('Repeat', rep, 'p-rep'), field('Repeat until', until, 'p-until')]));
    var kind = el('input', { type: 'checkbox', id: 'p-kind' }); kind.checked = c.kind === 'event';
    panel.appendChild(el('label', { for: 'p-kind' }, [kind, el('span', { text: ' Fixed event (the planner never moves it)' })]));
    // labels
    var chosen = c.labels.slice();
    var pick = el('div', { class: 'lblpick', role: 'group', 'aria-label': 'Colour labels' });
    LABELS.forEach(function (l) {
      var b = el('button', { type: 'button', class: 'lblbtn ' + l, text: l, 'aria-pressed': chosen.indexOf(l) >= 0 ? 'true' : 'false' });
      b.addEventListener('click', function () {
        var i = chosen.indexOf(l);
        if (i >= 0) { chosen.splice(i, 1); } else { chosen.push(l); }
        b.setAttribute('aria-pressed', i >= 0 ? 'false' : 'true');
      });
      pick.appendChild(b);
    });
    panel.appendChild(el('label', { text: 'Labels' }));
    panel.appendChild(pick);
    // checklist
    var checks = c.checklist.map(function (x) { return { t: x.t, d: x.d }; });
    var box = el('div');
    var prog = el('div', { class: 'bar2' }, [el('i')]);
    panel.appendChild(el('label', { text: 'Checklist' }));
    panel.appendChild(prog);
    panel.appendChild(box);
    function paintChecks() {
      box.textContent = '';
      var n = checks.filter(function (x) { return x.d; }).length;
      $('i', prog).style.width = (checks.length ? Math.round(100 * n / checks.length) : 0) + '%';
      checks.forEach(function (x, i) {
        var cb = el('input', { type: 'checkbox', 'aria-label': 'Done: ' + x.t }); cb.checked = x.d;
        var tx = el('input', { type: 'text', maxlength: '200', 'aria-label': 'Checklist item' }); tx.value = x.t;
        var rm = el('button', { type: 'button', text: '\u00d7', 'aria-label': 'Remove item', class: 'small' });
        cb.addEventListener('change', function () { x.d = cb.checked; paintChecks(); });
        tx.addEventListener('input', function () { x.t = tx.value; });
        rm.addEventListener('click', function () { checks.splice(i, 1); paintChecks(); });
        box.appendChild(el('div', { class: 'chkrow' + (x.d ? ' done' : '') }, [cb, tx, rm]));
      });
    }
    paintChecks();
    var newCheck = el('input', { type: 'text', maxlength: '200', placeholder: 'Add an item and press Enter', 'aria-label': 'New checklist item' });
    newCheck.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' && newCheck.value.trim()) { e.preventDefault(); checks.push({ t: newCheck.value.trim(), d: false }); newCheck.value = ''; paintChecks(); newCheck.focus(); }
    });
    panel.appendChild(newCheck);
    // move
    var ls = el('select'); fillLists(ls, c.list_id);
    var ps = el('select'); fillPositions(ps, c.list_id, c.id, c.index);
    ls.addEventListener('change', function () { fillPositions(ps, ls.value, c.id, 0); });
    panel.appendChild(el('div', { class: 'two' }, [field('Move to list', ls, 'p-list'), field('Position', ps, 'p-pos')]));
    // buttons
    var save = el('button', { type: 'button', class: 'primary', text: 'Save' });
    var arch = el('button', { type: 'button', text: 'Archive card' });
    var del = el('button', { type: 'button', class: 'danger', text: 'Delete card' });
    var armed = false;
    del.addEventListener('click', function () {
      if (!armed) { armed = true; del.textContent = 'Click again to delete'; return; }
      api('/api/delete', { ids: [c.id] }).then(function (r) { closePanel(); return handle(r); }).catch(fail);
    });
    arch.addEventListener('click', function () {
      api('/api/cards/' + c.id + '/archive', {}).then(function (r) { closePanel(); return handle(r); }).catch(fail);
    });
    save.addEventListener('click', function () {
      var body = {
        title: title.value, notes: notes.value, priority: parseInt(pri.value, 10), labels: chosen, checklist: checks,
        date: date.value || null, time: time.value || null, reminder_min: rem.value === '' ? null : parseInt(rem.value, 10),
        repeat: rep.value.trim() || null, until: until.value || null, kind: kind.checked ? 'event' : 'task', dur_min: c.dur_min
      };
      api('/api/cards/' + c.id + '/update', body).then(function (r) {
        var moved = String(ls.value) !== String(c.list_id) || parseInt(ps.value, 10) !== c.index;
        closePanel();
        if (moved) { return moveCards([{ id: c.id, list: parseInt(ls.value, 10), index: parseInt(ps.value, 10) }]); }
        return handle(r);
      }).catch(fail);
    });
    panel.appendChild(el('div', { class: 'panelfoot' }, [save, arch, del]));
    panel.hidden = false;
    title.focus();
  }

  // ---- what should I do now? ----
  var nowDlg = $('#now-dialog'), nowCard = null;
  $('#now-btn').addEventListener('click', function () {
    api('/api/now').then(function (r) {
      var b = $('#now-body'); b.textContent = '';
      nowCard = r.clear ? null : r.id;
      if (r.clear) {
        b.appendChild(el('p', { class: 'big', text: "You're clear" }));
        b.appendChild(el('p', { class: 'muted', text: 'Nothing needs you right now. Enjoy the quiet, or add a card.' }));
      } else {
        b.appendChild(el('p', { class: 'big', text: r.title }));
        b.appendChild(el('p', { class: 'muted', text: r.why + ' ' + r.when }));
      }
      $('#now-open').hidden = $('#now-done').hidden = !nowCard;
      openDlg(nowDlg);
    }).catch(fail);
  });
  $('#now-close').addEventListener('click', function () { closeDlg(nowDlg); });
  $('#now-open').addEventListener('click', function () { closeDlg(nowDlg); openPanel(nowCard); });
  $('#now-done').addEventListener('click', function () {
    closeDlg(nowDlg);
    api('/api/tasks/' + nowCard + '/done', {}).then(handle).catch(fail);
  });

  // ---- move dialogs ----
  var cmDlg = $('#cardmove-dialog'), cmId = null;
  var maDlg = $('#moveall-dialog'), maFrom = null;
  var dlg = $('#move-dialog'), moving = null;
  function pad(n) { return (n < 10 ? '0' : '') + n; }
  function openMove(id, title, at) {
    moving = parseInt(id, 10);
    $('#move-title').textContent = title || '';
    if (!at) {
      var d = new Date(Date.now() + 3600000);
      at = d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate()) + 'T' + pad(d.getHours()) + ':00';
    }
    $('#move-when').value = at;
    openDlg(dlg);
    $('#move-when').focus();
  }
  function sendMove(id, to) { api('/api/move', { moves: [{ id: id, to: to }] }).then(handle).catch(fail); }
  $('#move-ok').addEventListener('click', function () {
    var v = $('#move-when').value;
    if (!v) { toast('Pick a date and time.'); return; }
    closeDlg(dlg); sendMove(moving, v);
  });
  $('#move-cancel').addEventListener('click', function () { closeDlg(dlg); });
  $('#cm-list').addEventListener('change', function () { fillPositions($('#cm-pos'), $('#cm-list').value, cmId, 0); });
  $('#cm-ok').addEventListener('click', function () {
    closeDlg(cmDlg);
    moveCards([{ id: parseInt(cmId, 10), list: parseInt($('#cm-list').value, 10), index: parseInt($('#cm-pos').value, 10) }]);
  });
  $('#cm-cancel').addEventListener('click', function () { closeDlg(cmDlg); });
  $('#ma-ok').addEventListener('click', function () {
    closeDlg(maDlg);
    api('/api/lists/move-all', { from: parseInt(maFrom, 10), to: parseInt($('#ma-list').value, 10) }).then(handle).catch(fail);
  });
  $('#ma-cancel').addEventListener('click', function () { closeDlg(maDlg); });

  // ---- clicks (delegated, so they survive refresh) ----
  document.addEventListener('click', function (e) {
    var t = e.target;
    var b = t.closest('button');
    if (b) {
      var c = b.classList;
      if (c.contains('add-open')) {
        var f = $('.add-form', b.parentNode); f.hidden = false; b.hidden = true; $('.add-text', f).focus();
      } else if (c.contains('add-read')) { readCard(b.closest('.add-form'));
      } else if (c.contains('add-save')) { saveCard(b.closest('.add-form'));
      } else if (c.contains('add-cancel')) { closeAdder(b.closest('.add-form'));
      } else if (b.id === 'addlist-open') { b.hidden = true; $('#addlist-form').hidden = false; $('#addlist-name').focus();
      } else if (b.id === 'addlist-cancel') { $('#addlist-form').hidden = true; $('#addlist-open').hidden = false;
      } else if (c.contains('l-left') || c.contains('l-right')) {
        var ids = $$('.list[data-id]').map(function (x) { return x.dataset.id; });
        var at = ids.indexOf(b.dataset.id) + (c.contains('l-left') ? -1 : 1);
        api('/api/lists/' + b.dataset.id + '/move', { index: Math.max(0, at) }).then(handle).catch(fail);
      } else if (c.contains('l-archive')) {
        api('/api/lists/' + b.dataset.id + '/archive', {}).then(handle).catch(fail);
      } else if (c.contains('l-moveall')) {
        maFrom = b.dataset.id;
        $('#ma-title').textContent = 'Move every card from "' + b.dataset.name + '" to another list.';
        fillLists($('#ma-list'), null, b.dataset.id);
        $$('details.lmenu').forEach(function (d) { d.open = false; });
        openDlg(maDlg);
      } else if (c.contains('cmove')) {
        cmId = b.dataset.id;
        var card = b.closest('.bcard'), cur = card.parentNode.dataset.list;
        $('#cm-title').textContent = b.dataset.title;
        fillLists($('#cm-list'), cur);
        var idx = cardsIn(cur, cmId).filter(function (x) { return x.compareDocumentPosition(card) & 4; }).length;
        fillPositions($('#cm-pos'), cur, cmId, idx);
        openDlg(cmDlg);
      } else if (c.contains('open')) { openPanel(b.dataset.id);
      } else if (c.contains('move')) { openMove(b.dataset.id, b.dataset.title, b.dataset.at);
      } else if (c.contains('approve')) { api('/api/proposals/' + b.dataset.pid + '/approve', {}).then(handle).catch(fail);
      } else if (c.contains('cancel')) { api('/api/proposals/' + b.dataset.pid + '/cancel', {}).then(function (r) { toast(r.message); location.reload(); }).catch(fail);
      } else if (c.contains('undo')) { api('/api/undo', { id: parseInt(b.dataset.uid, 10) }).then(function () { location.reload(); }).catch(fail);
      } else if (b.id === 'sample') { api('/api/sample', {}).then(handle).catch(fail);
      } else if (b.id === 'f-clear') { $('#q').value = ''; $('#f-label').value = ''; $('#f-due').value = ''; applyFilter();
      } else if (b.id === 'plan-day') { api('/api/plan', { missed: false }).then(handle).catch(fail);
      } else if (b.id === 'plan-missed') { api('/api/plan', { missed: true }).then(handle).catch(fail);
      } else if (b.id === 'notify') {
        if (!('Notification' in window)) { toast('This browser cannot show pop-ups.'); return; }
        Notification.requestPermission().then(function (p) {
          toast(p === 'granted' ? 'Pop-up reminders are on while this page is open.' : 'Pop-ups are off. Reminders still show in the yellow bar.');
        });
      }
      return;
    }
    var card2 = t.closest('.bcard');
    if (card2) { openPanel(card2.dataset.id); return; }
    if (!t.closest('details.lmenu')) { $$('details.lmenu').forEach(function (d) { d.open = false; }); }
  });
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape' && !panel.hidden) { closePanel(); return; }
    var t = e.target;
    if ((e.key === 'Enter' || e.key === ' ') && t.classList && t.classList.contains('bcard')) { e.preventDefault(); openPanel(t.dataset.id); }
    if (e.key === 'Enter' && !e.shiftKey && t.classList && t.classList.contains('add-text')) {
      e.preventDefault();
      var form = t.closest('.add-form');
      if (form._parsed && form._text === t.value) { saveCard(form); } else { readCard(form); }
    }
    if (e.key === 'Escape' && t.classList && t.classList.contains('add-text')) { closeAdder(t.closest('.add-form')); }
  });
  document.addEventListener('input', function (e) {
    if (e.target.id === 'q') { applyFilter(); }
    if (e.target.classList.contains('add-text')) { var f = e.target.closest('.add-form'); f._parsed = null; $('.add-save', f).hidden = true; }
  });
  document.addEventListener('change', function (e) {
    var t = e.target;
    if (t.id === 'f-label' || t.id === 'f-due') { applyFilter(); }
    if (t.classList.contains('lname')) {
      api('/api/lists/' + t.dataset.id + '/rename', { name: t.value }).then(handle).catch(function (er) { fail(er); refresh(); });
    }
  });
  document.addEventListener('submit', function (e) {
    if (e.target.id === 'addlist-form') {
      e.preventDefault();
      api('/api/lists', { name: $('#addlist-name').value }).then(handle).catch(fail);
    }
  });

  // ---- drag and drop: cards between lists, calendar items onto an hour ----
  var dragId = null, dragBoardCard = false;
  var line = el('div', { class: 'drop-line' });
  function dropIndex(container, y) {
    var kids = $$('.bcard', container).filter(function (c) { return !c.classList.contains('dragging'); });
    var idx = kids.length;
    for (var i = 0; i < kids.length; i++) {
      var r = kids[i].getBoundingClientRect();
      if (y < r.top + r.height / 2) { idx = i; break; }
    }
    return { idx: idx, before: kids[idx] || null };
  }
  document.addEventListener('dragstart', function (e) {
    var item = e.target.closest && e.target.closest('[data-id][draggable=true]');
    if (!item) { return; }
    dragId = parseInt(item.dataset.id, 10);
    dragBoardCard = item.classList.contains('bcard');
    e.dataTransfer.setData('text/plain', item.dataset.id);
    e.dataTransfer.effectAllowed = 'move';
    if (dragBoardCard) { setTimeout(function () { item.classList.add('dragging'); }, 0); }
  });
  document.addEventListener('dragend', function () {
    $$('.dragging').forEach(function (x) { x.classList.remove('dragging'); });
    if (line.parentNode) { line.parentNode.removeChild(line); }
    $$('.over').forEach(function (x) { x.classList.remove('over'); });
    dragId = null;
  });
  document.addEventListener('dragover', function (e) {
    var board = e.target.closest && e.target.closest('#board');
    if (board && dragBoardCard) {
      var r = board.getBoundingClientRect();
      if (e.clientX < r.left + 50) { board.scrollLeft -= 18; } else if (e.clientX > r.right - 50) { board.scrollLeft += 18; }
    }
    var list = e.target.closest && e.target.closest('.list[data-id]');
    if (list && dragBoardCard) {
      e.preventDefault();
      var cont = $('.cards', list), d = dropIndex(cont, e.clientY);
      cont.insertBefore(line, d.before);
      return;
    }
    var slot = e.target.closest && e.target.closest('.slot');
    if (slot && dragId && !dragBoardCard) { e.preventDefault(); slot.classList.add('over'); }
  });
  document.addEventListener('dragleave', function (e) {
    var slot = e.target.closest && e.target.closest('.slot');
    if (slot) { slot.classList.remove('over'); }
  });
  document.addEventListener('drop', function (e) {
    var list = e.target.closest && e.target.closest('.list[data-id]');
    if (list && dragBoardCard && dragId) {
      e.preventDefault();
      var cont = $('.cards', list), d = dropIndex(cont, e.clientY), id = dragId;
      if (line.parentNode) { line.parentNode.removeChild(line); }
      moveCards([{ id: id, list: parseInt(list.dataset.id, 10), index: d.idx }]);
      return;
    }
    var slot = e.target.closest && e.target.closest('.slot');
    if (slot && dragId && !dragBoardCard) {
      e.preventDefault();
      slot.classList.remove('over');
      sendMove(dragId, slot.dataset.slot);
    }
  });

  // ---- reminders ----
  var shown = {};
  function paintBanner(list) {
    var b = $('#banner');
    b.textContent = '';
    if (!list.length) { return; }
    b.appendChild(el('div', { text: list.length === 1 ? '1 reminder:' : list.length + ' reminders:' }));
    var ul = el('ul');
    list.forEach(function (d) { ul.appendChild(el('li', { text: d.title + (d.when ? ' (' + d.when + ')' : '') })); });
    var btn = el('button', { type: 'button', text: 'Dismiss' });
    btn.addEventListener('click', function () {
      api('/api/due/ack', { ids: list.map(function (d) { return d.id; }) }).then(poll).catch(fail);
    });
    b.appendChild(ul); b.appendChild(btn);
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
  poll();
  setInterval(poll, 30000);
  applyFilter();
})();
