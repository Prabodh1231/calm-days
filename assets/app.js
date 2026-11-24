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
    $$('.cards[data-list]').forEach(function (cont) {
      var n = $$('.bcard', cont).filter(function (c) { return !c.hidden; }).length;
      var cnt = $('.count', cont.parentNode);
      if (cnt) { cnt.textContent = n; cnt.setAttribute('aria-label', n + (n === 1 ? ' card' : ' cards')); }
    });
    var nm = $('#nomatch'); if (nm) { nm.hidden = !(active && total > 0 && shown === 0); }
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
      if (window.__initCal) { window.__initCal(); }
      if (msg) { toast(msg, undoId); }
    }).catch(fail);
  }
  function handle(res) {
    if (res.status === 'proposal' && location.pathname !== '/plan') {
      return confirmProposal(res.proposal);
    } else if (res.status === 'proposal') {
      location.href = '/plan';
    } else if (res.status === 'none') {
      toast(res.message);
    } else {
      return refresh(res.message, res.undo);
    }
  }

  // ---- approval question, shown on the page the user is on ----
  var cfDlg = $('#confirm-dialog'), cfId = null;
  function confirmProposal(pid) {
    return api('/api/proposals/' + pid).then(function (r) {
      cfId = pid;
      var ul = $('#confirm-lines'); ul.textContent = '';
      r.lines.forEach(function (l) { ul.appendChild(el('li', { text: l })); });
      openDlg(cfDlg);
      $('#confirm-ok').focus();
    }).catch(fail);
  }
  function cancelProposal() {
    var id = cfId; cfId = null; closeDlg(cfDlg);
    if (id) { api('/api/proposals/' + id + '/cancel', {}).then(function (r) { toast(r.message); refresh(); }).catch(fail); }
  }
  $('#confirm-cancel').addEventListener('click', cancelProposal);
  cfDlg.addEventListener('cancel', function (e) { e.preventDefault(); cancelProposal(); });
  $('#confirm-ok').addEventListener('click', function () {
    var id = cfId; cfId = null; closeDlg(cfDlg);
    if (id) { api('/api/proposals/' + id + '/approve', {}).then(handle).catch(fail); }
  });

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
    var o = { weekday: 'short', day: 'numeric', month: 'short' };
    if (d.getFullYear() !== new Date().getFullYear()) { o.year = 'numeric'; }
    var s = d.toLocaleDateString(undefined, o);
    return p.time ? s + ', ' + d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) : s;
  }
  var PRI = ['Low', 'Medium', 'High'];
  var LNAME = { green: 'Money', red: 'Health', blue: 'People', purple: 'Work', orange: 'Home', yellow: 'Ideas' };
  // The chips for one reading. Text goes in with textContent. Dashed chips came from the AI; the first chip says who read it.
  function chipsFor(p, add) {
    var ai = p.ai_fields || [];
    var from = function (k) { return ai.indexOf(k) >= 0 ? 'fromai' : ''; };
    var secs = typeof p.ms === 'number' ? (p.ms / 1000).toFixed(1) + ' s' : '';
    if (p.source === 'ai') {
      add('AI', 'src ai', 'Read by the small AI on your computer (' + secs + '). Dashed chips came from the AI. Check them before you save.');
    } else {
      add('rules', 'src rules', 'Read by simple rules' + (p.reason ? ' (' + p.reason + ')' : '') + '. Check the chips before you save.');
    }
    if (p.source !== 'ai' && p.reason) {
      if (/reader is off/.test(p.reason)) { add('AI is off, so simple rules were used.', 'warnchip'); }
      else if (/nothing usable|could not use/.test(p.reason)) { add('AI could not read this one, rules were used.', 'warnchip'); }
      else { add('AI is not ready yet. Open AI reader and press Check, then try again.', 'warnchip'); }
    }
    if (p.date) { add('Due ' + fmtDate(p), from('date') + ' ' + from('time')); } else { add('No date found', 'warnchip'); }
    add('Priority: ' + PRI[p.priority], from('priority'));
    if (p.repeat) { add('Repeats: ' + p.repeat, from('repeat')); }
    if (p.reminder_min === -1) { add('No reminder', from('reminder')); } else if (p.date) { add(p.reminder_text || 'Reminder: 10 min before', from('reminder')); }
    if (p.dur_min && p.date && p.time) { add(p.dur_min + ' min', from('duration')); }
    (p.labels || []).forEach(function (l) { add('Label: ' + (LNAME[l] || l), from('labels')); });
    if (p.kind === 'event') { add('Fixed event'); }
    (p.notes || []).forEach(function (n) { add(n, 'warnchip'); });
  }
  function showPreview(form, p) {
    var pv = $('.prev', form);
    pv.textContent = '';
    var add = function (t, cls, tip) {
      var c = el('span', { class: 'chip' + (cls && cls.trim() ? ' ' + cls.trim() : ''), text: t });
      if (tip) { c.title = tip; }
      pv.appendChild(c);
    };
    // The title is a box you can change before saving.
    var tb = el('input', { type: 'text', class: 'titlebox', maxlength: '200', 'aria-label': 'Card title (you can change it)' });
    tb.value = p.title || '';
    tb.addEventListener('input', function () { p.title = tb.value; });
    pv.appendChild(tb);
    chipsFor(p, add);
    form._parsed = p;
    form._text = $('.add-text', form).value;
    $('.add-save', form).hidden = false;
  }
  var warmed = false;
  function warmAi() { if (!warmed) { warmed = true; api('/api/ai/warm', {}).catch(function () { /* optional */ }); } }
  function readCard(form) {
    var v = $('.add-text', form).value.trim();
    if (!v) { toast('Type a sentence first, for example: Remind me to call John tomorrow at 5 PM'); return; }
    var pv = $('.prev', form); pv.textContent = ''; pv.appendChild(el('span', { class: 'chip', text: 'Reading your sentence...' }));
    api('/api/parse', { text: v }).then(function (p) { showPreview(form, p); }).catch(function (e) { pv.textContent = ''; fail(e); });
  }
  function saveCard(form) {
    var p = form._parsed;
    if (!p) { return; }
    api('/api/tasks', {
      title: p.title, date: p.date, time: p.time, priority: p.priority, reminder_min: p.reminder_min,
      repeat: p.repeat || null, until: p.until, dur_min: p.dur_min, kind: p.kind, labels: p.labels || [],
      list_id: parseInt($('.add-open', form.parentNode).dataset.list, 10)
    }).then(function (r) {
      return refresh('Card added.', r.undo).then(function () {
        var c = $('.bcard[data-id="' + r.id + '"]');
        if (c) { c.scrollIntoView({ block: 'nearest', inline: 'nearest' }); c.classList.add('flash'); setTimeout(function () { c.classList.remove('flash'); }, 2200); }
      });
    }).catch(fail);
  }
  function closeAdder(form) {
    form.hidden = true; form._parsed = null;
    $('.prev', form).textContent = ''; $('.add-text', form).value = ''; $('.add-save', form).hidden = true;
    $('.add-open', form.parentNode).hidden = false;
  }

  // ---- card detail panel ----
  var panel = $('#panel'), panelId = null;
  var panelIdRef = null;
  // Close with unsaved edits asks first (a bar above the buttons); Keep editing leaves everything as it was.
  function askDiscard() {
    if ($('.discard', panel)) { return; }
    var keep = el('button', { type: 'button', text: 'Keep editing' }), drop = el('button', { type: 'button', class: 'danger', text: 'Discard' });
    keep.addEventListener('click', function () { $('.discard', panel).remove(); });
    drop.addEventListener('click', closePanel);
    var bar = el('div', { class: 'discard', role: 'alert' }, [el('b', { text: 'Discard changes?' }), keep, drop]);
    panel.insertBefore(bar, $('.panelfoot', panel));
    keep.focus();
  }
  function closePanel() { panel.hidden = true; panel.textContent = ''; panelId = null; }
  var LABELS = ['green', 'yellow', 'orange', 'red', 'purple', 'blue'];
  function field(label, input, id) {
    input.id = id;
    return el('div', {}, [el('label', { for: id, text: label }), input]);
  }
  function option(v, t, sel) { var o = el('option', { value: v, text: t }); if (sel) { o.selected = true; } return o; }
  function openPanel(id) {
    api('/api/cards/' + id).then(function (c) { buildPanel(c); }).catch(fail);
  }
  var LTITLE = { green: 'Money', red: 'Health', blue: 'People', purple: 'Work', orange: 'Home', yellow: 'Ideas' };
  function group(title, kids) { return el('div', { class: 'pgroup' }, [el('h3', { text: title })].concat(kids)); }
  function buildPanel(c) {
    panelId = c.id;
    panel.textContent = '';
    var closeB = el('button', { type: 'button', text: 'Close', 'aria-label': 'Close card details' });
    closeB.addEventListener('click', function () { if (panel._dirty && panel._dirty()) { askDiscard(); } else { closePanel(); } });
    panel.appendChild(el('h2', {}, [el('span', {}, [el('span', { class: 'cid', text: c.cid }), el('span', { text: c.done ? ' Card (done)' : ' Card' })]), closeB]));
    var main = el('div', { class: 'pmain' }), side = el('aside', { class: 'pside', 'aria-label': 'Properties' });
    panel.appendChild(el('div', { class: 'pgrid' }, [main, side]));
    var title = el('input', { type: 'text', maxlength: '200' }); title.value = c.title;
    main.appendChild(field('Title', title, 'p-title'));
    var notes = el('textarea', { rows: '4', maxlength: '5000', placeholder: 'Add notes...' }); notes.value = c.notes;
    main.appendChild(field('Notes', notes, 'p-notes'));
    // checklist
    var checks = c.checklist.map(function (x) { return { t: x.t, d: x.d }; });
    var box = el('div');
    var prog = el('div', { class: 'bar2' }, [el('i')]);
    main.appendChild(el('label', { text: 'Checklist' }));
    main.appendChild(prog);
    main.appendChild(box);
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
    main.appendChild(newCheck);
    // activity
    var act = el('ul', { class: 'activity', 'aria-label': 'Activity' });
    if (!c.activity.length) { act.appendChild(el('li', { class: 'muted', text: 'No activity yet.' })); }
    c.activity.forEach(function (a) { act.appendChild(el('li', {}, [el('span', { text: a.text }), el('span', { class: 'ago', text: a.ago })])); });
    main.appendChild(el('h3', { text: 'Activity' }));
    main.appendChild(act);

    // ---- properties sidebar ----
    var ls = el('select'); fillLists(ls, c.list_id);
    var ps = el('select'); fillPositions(ps, c.list_id, c.id, c.index);
    ls.addEventListener('change', function () { fillPositions(ps, ls.value, c.id, 0); });
    side.appendChild(field('Status (list)', ls, 'p-list'));
    side.appendChild(field('Position', ps, 'p-pos'));
    var pri = el('select', {}, [option('0', 'Low', c.priority === 0), option('1', 'Medium', c.priority === 1), option('2', 'High', c.priority === 2)]);
    side.appendChild(field('Priority', pri, 'p-pri'));
    var date = el('input', { type: 'date' }); date.value = c.date || '';
    var time = el('input', { type: 'time' }); time.value = c.time || '';
    side.appendChild(el('div', { class: 'two stack' }, [field('Due date', date, 'p-date'), field('Due time', time, 'p-time')]));
    var dur = el('input', { type: 'number', min: '5', max: '1440', step: '5' }); dur.value = c.dur_min;
    side.appendChild(field('Length (minutes)', dur, 'p-dur'));
    // labels with names, plus suggestions that only apply when accepted
    var chosen = c.labels.slice();
    var pick = el('div', { class: 'lblpick', role: 'group', 'aria-label': 'Labels' });
    var btns = {};
    LABELS.forEach(function (l) {
      var b = el('button', { type: 'button', class: 'lblbtn ' + l, text: LTITLE[l], 'aria-pressed': chosen.indexOf(l) >= 0 ? 'true' : 'false' });
      b.addEventListener('click', function () {
        var i = chosen.indexOf(l);
        if (i >= 0) { chosen.splice(i, 1); } else { chosen.push(l); }
        b.setAttribute('aria-pressed', i >= 0 ? 'false' : 'true');
      });
      btns[l] = b;
      pick.appendChild(b);
    });
    side.appendChild(el('label', { text: 'Labels' }));
    side.appendChild(pick);
    (c.suggest || []).forEach(function (l) {
      var chip = el('button', { type: 'button', class: 'suggest', text: 'Suggested label: ' + LTITLE[l] + '. Add it?', title: 'Click to add this label. Nothing is added until you click.' });
      chip.addEventListener('click', function () {
        if (chosen.indexOf(l) < 0) { chosen.push(l); }
        btns[l].setAttribute('aria-pressed', 'true');
        chip.remove();
      });
      side.appendChild(chip);
    });
    var rep = el('input', { type: 'text', placeholder: 'none, or: every week on mon' }); rep.value = c.repeat || '';
    var until = el('input', { type: 'date' }); until.value = c.until || '';
    side.appendChild(field('Repeat', rep, 'p-rep'));
    side.appendChild(field('Repeat until', until, 'p-until'));
    var rem = el('select', {}, [option('', 'Default', c.reminder_min === null), option('5', '5 min before', c.reminder_min === 5), option('30', '30 min before', c.reminder_min === 30),
      option('60', '1 hour before', c.reminder_min === 60), option('1440', '1 day before', c.reminder_min === 1440), option('-1', 'No reminder', c.reminder_min === -1)]);
    side.appendChild(field('Reminder', rem, 'p-rem'));
    var cyc = el('select', {}, [option('none', 'Not planned', c.cycle === ''), option('this', 'This week', c.cycle === 'this'), option('next', 'Next week', c.cycle === 'next')]);
    if (c.cycle === 'past' || c.cycle === 'later') { cyc.appendChild(option('past', c.cycle === 'past' ? 'Earlier week' : 'Later week', true)); }
    side.appendChild(field('Plan for', cyc, 'p-cycle'));
    var kind = el('input', { type: 'checkbox', id: 'p-kind' }); kind.checked = c.kind === 'event';
    side.appendChild(el('label', { for: 'p-kind' }, [kind, el('span', { text: ' Fixed event (the planner never moves it)' })]));

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
        repeat: rep.value.trim() || null, until: until.value || null, kind: kind.checked ? 'event' : 'task',
        dur_min: parseInt(dur.value, 10) || c.dur_min, cycle: cyc.value
      };
      api('/api/cards/' + c.id + '/update', body).then(function (r) {
        var moved = String(ls.value) !== String(c.list_id) || parseInt(ps.value, 10) !== c.index;
        closePanel();
        if (moved) { return moveCards([{ id: c.id, list: parseInt(ls.value, 10), index: parseInt(ps.value, 10) }]); }
        return handle(r);
      }).catch(fail);
    });
    panel.appendChild(el('div', { class: 'panelfoot' }, [save, arch, del]));
    function snap() { return JSON.stringify([title.value, notes.value, pri.value, date.value, time.value, rem.value, rep.value, until.value, kind.checked, dur.value, cyc.value, chosen.slice().sort(), checks, ls.value, ps.value]); }
    var snap0 = snap();
    panel._dirty = function () { return snap() !== snap0; };
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
        var f = $('.add-form', b.parentNode); f.hidden = false; b.hidden = true; $('.add-text', f).focus(); warmAi();
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
      } else if (b.id === 'f-clear' || b.id === 'nm-clear') { $('#q').value = ''; $('#f-label').value = ''; $('#f-due').value = ''; applyFilter();
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
    var head = el('div', { class: 'bn-head' });
    head.appendChild(el('b', { text: list.length === 1 ? '1 reminder' : list.length + ' reminders' }));
    var ul = el('ul');
    list.forEach(function (d) { ul.appendChild(el('li', { text: d.title + (d.when ? ' (' + d.when + ')' : '') })); });
    var btn = el('button', { type: 'button', class: 'small', text: 'Dismiss' });
    btn.addEventListener('click', function () {
      api('/api/due/ack', { ids: list.map(function (d) { return d.id; }) }).then(poll).catch(fail);
    });
    head.appendChild(btn); b.appendChild(head); b.appendChild(ul);
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

  // ================= Linear-style features =================
  // phone: the calendar opens as an agenda list
  if (location.pathname === '/calendar' && location.search.indexOf('view=') < 0 && window.innerWidth < 700) { location.replace('/calendar?view=agenda'); }

  // ---- the card you are on: focused card, open panel, or the one under the mouse ----
  var hoverId = null;
  document.addEventListener('mouseover', function (e) {
    var c = e.target.closest && e.target.closest('[data-id].bcard,[data-id].evb,[data-id].mpill,[data-id].ag-item,[data-id].adpill');
    if (c) { hoverId = parseInt(c.dataset.id, 10); }
  });
  function selectedId() {
    var a = document.activeElement;
    var c = a && a.closest && a.closest('[data-id].bcard,[data-id].evb,[data-id].mpill,[data-id].ag-item,[data-id].adpill');
    if (c) { return parseInt(c.dataset.id, 10); }
    return panelId || hoverId;
  }

  // ---- command menu (Ctrl+K / Cmd+K) ----
  var ck = $('#cmdk'), ckIn = $('#cmdk-input'), ckList = $('#cmdk-list');
  var ckMode = 'main', ckRows = [], ckSel = 0, ckData = { cards: [], lists: [] }, ckMoveId = null;
  function score(q, t) {
    q = q.toLowerCase(); t = t.toLowerCase();
    if (!q) { return 1; }
    var i = t.indexOf(q);
    if (i >= 0) { return 1000 - i - (t.length - q.length) / 100; }
    var p = 0, gaps = 0, last = -1;
    for (var k = 0; k < t.length && p < q.length; k++) {
      if (t[k] === q[p]) { if (last >= 0 && k - last > 1) { gaps++; } last = k; p++; }
    }
    return p === q.length ? 500 - gaps * 10 - t.length / 100 : -1;
  }
  function go(url) { location.href = url; }
  function actions() {
    var sel = selectedId();
    return [
      { label: 'New card', hint: 'goes to the Inbox', run: function () { setMode('capture'); } },
      { label: 'Go to Board', hint: 'G then B', run: function () { go('/'); } },
      { label: 'Go to Calendar', hint: 'G then C', run: function () { go('/calendar'); } },
      { label: 'Go to Summary', hint: 'G then S', run: function () { go('/summary'); } },
      { label: 'Toggle dark mode', hint: '', run: function () { themeBtn.click(); closeCk(); } },
      { label: 'Move selected card', hint: sel ? 'CD-' + sel : 'focus or open a card first', run: function () { if (sel) { ckMoveId = sel; setMode('move'); } else { toast('Focus or open a card first, then try again.'); closeCk(); } } },
      { label: 'Plan my day', hint: 'asks you first', run: function () { closeCk(); api('/api/plan', { missed: false }).then(handle).catch(fail); } },
      { label: 'What should I do now?', hint: '', run: function () { closeCk(); $('#now-btn').click(); } },
      { label: 'Show keyboard shortcuts', hint: '?', run: function () { closeCk(); openDlg($('#help-dialog')); } }
    ];
  }
  function setMode(m) {
    ckMode = m; ckIn.value = '';
    ckIn.placeholder = m === 'capture' ? 'Type a card, e.g. call John tomorrow at 5 PM, then press Enter' : m === 'move' ? 'Move CD-' + ckMoveId + ' to which list?' : 'Type a command or search cards...';
    paintCk();
    ckIn.focus();
  }
  function paintCk() {
    var q = ckIn.value.trim(), rows = [];
    if (ckMode === 'preview') {
      if (!ckPrev) {
        rows.push({ label: 'Reading your sentence...', hint: '', run: function () {} });
      } else {
        var box = el('div', { class: 'cmdk-prev prev' });
        var tb = el('input', { type: 'text', class: 'titlebox', maxlength: '200', 'aria-label': 'Card title (you can change it)' });
        tb.value = ckPrev.title || '';
        tb.addEventListener('input', function () { ckPrev.title = tb.value; });
        tb.addEventListener('keydown', function (e) { e.stopPropagation(); if (e.key === 'Enter') { e.preventDefault(); saveCapture(ckPrev); } else if (e.key === 'Escape') { closeCk(); } });
        box.appendChild(tb);
        chipsFor(ckPrev, function (t, cls, tip) { var c = el('span', { class: 'chip' + (cls && cls.trim() ? ' ' + cls.trim() : ''), text: t }); if (tip) { c.title = tip; } box.appendChild(c); });
        rows.push({ node: box, run: function () {} });
        rows.push({ label: 'Add to the Inbox', hint: 'Enter', run: function () { saveCapture(ckPrev); } });
        rows.push({ label: 'Back: change the sentence', hint: '', run: function () { ckMode = 'capture'; ckPrev = null; ckSel = 0; paintCk(); ckIn.focus(); } });
      }
    } else if (ckMode === 'capture') {
      rows.push({ label: q ? 'Add "' + q + '" to the Inbox' : 'Type a sentence, then press Enter', hint: 'Inbox', run: function () { if (q) { captureCard(q); } } });
    } else if (ckMode === 'move') {
      ckData.lists.forEach(function (l) {
        var s = score(q, l.name);
        if (s >= 0) { rows.push({ s: s, label: 'Move to ' + l.name, hint: 'CD-' + ckMoveId, run: function () { closeCk(); moveCards([{ id: ckMoveId, list: l.id, index: null }]); } }); }
      });
    } else {
      actions().forEach(function (a) { var s = score(q, a.label); if (s >= 0) { a.s = s + 2000; rows.push(a); } });
      var cards = [];
      ckData.cards.forEach(function (c) {
        var s = Math.max(score(q, c.cid + ' ' + c.title), score(q, c.title));
        if (q && s >= 0) { cards.push({ s: s, label: c.title, hint: c.cid + ' in ' + c.list, run: function () { closeCk(); openPanel(c.id); } }); }
      });
      cards.sort(function (a, b) { return b.s - a.s; });
      rows = rows.sort(function (a, b) { return b.s - a.s; }).concat(cards.slice(0, 8));
      if (q) { rows.push({ label: 'Add "' + q + '" to the Inbox', hint: 'new card', run: function () { captureCard(q); } }); }
      if (q && rows.length === 1) { ckSel = 0; }
    }
    ckRows = rows;
    if (ckSel >= rows.length) { ckSel = 0; }
    ckList.textContent = '';
    rows.forEach(function (r, i) {
      var li = el('li', { role: 'option', 'aria-selected': i === ckSel ? 'true' : 'false', class: i === ckSel ? 'sel' : '' }, r.node ? [r.node] : [el('span', { text: r.label }), el('span', { class: 'muted', text: r.hint || '' })]);
      li.addEventListener('mousedown', function (e) { e.preventDefault(); ckSel = i; r.run(); });
      ckList.appendChild(li);
    });
    var s = $('li.sel', ckList); if (s && s.scrollIntoView) { s.scrollIntoView({ block: 'nearest' }); }
  }
  function openCk(mode) {
    ck.hidden = false; ckSel = 0; ckMoveId = selectedId();
    if (mode === 'capture') { warmAi(); }
    ckData = { cards: [], lists: [] };
    setMode(mode || 'main');
    api('/api/cards/index').then(function (r) { ckData = r; paintCk(); }).catch(function () { /* menu still works */ });
  }
  function closeCk() { ck.hidden = true; }
  function captureCard(text) {
    // Read first, show the chips, save only when the user presses Enter on "Add to the Inbox".
    ckMode = 'preview'; ckPrev = null; ckSel = 0;
    ckIn.value = text; ckIn.placeholder = '';
    ckRows = [{ label: 'Reading your sentence...', hint: '', run: function () {} }];
    paintCk();
    api('/api/parse', { text: text }).then(function (p) {
      if (ck.hidden) { return; }
      ckPrev = p; ckSel = 1; paintCk();
    }).catch(function (e) { closeCk(); fail(e); });
  }
  function saveCapture(p) {
    closeCk();
    api('/api/tasks', { title: p.title, date: p.date, time: p.time, priority: p.priority, reminder_min: p.reminder_min, repeat: p.repeat || null, until: p.until, dur_min: p.dur_min, kind: p.kind, labels: p.labels || [] })
      .then(function (r) { return refresh('Added to your Inbox.', r.undo); }).catch(fail);
  }
  var ckPrev = null;
  ckIn.addEventListener('input', function () { if (ckMode === 'preview') { ckMode = 'capture'; ckPrev = null; } ckSel = 0; paintCk(); });
  ckIn.addEventListener('keydown', function (e) {
    if (e.key === 'ArrowDown') { e.preventDefault(); ckSel = Math.min(ckRows.length - 1, ckSel + 1); paintCk(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); ckSel = Math.max(0, ckSel - 1); paintCk(); }
    else if (e.key === 'Enter') { e.preventDefault(); if (ckRows[ckSel]) { ckRows[ckSel].run(); } }
    else if (e.key === 'Escape') { e.preventDefault(); closeCk(); }
  });
  ck.addEventListener('mousedown', function (e) { if (e.target === ck) { closeCk(); } });
  $('#cmdk-btn').addEventListener('click', function () { openCk(); });
  $('#help-close').addEventListener('click', function () { closeDlg($('#help-dialog')); });

  // ---- AI reader settings ----
  var aiDlg = $('#ai-dialog'), aiSt = $('#ai-status');
  function aiSay(t, cls) { aiSt.textContent = t; aiSt.className = 'muted' + (cls ? ' ' + cls : ''); }
  $('#ai-btn').addEventListener('click', function () {
    aiSay('');
    api('/api/ai').then(function (s) {
      $('#ai-on').checked = !!s.on; $('#ai-model').value = s.model; $('#ai-url').value = s.url;
      openDlg(aiDlg);
      if (s.on) { aiCheck(); }
    }).catch(fail);
  });
  $('#ai-close').addEventListener('click', function () { closeDlg(aiDlg); });
  function aiSave() {
    return api('/api/ai', { on: $('#ai-on').checked, model: $('#ai-model').value, url: $('#ai-url').value });
  }
  $('#ai-save').addEventListener('click', function () {
    aiSave().then(function () { aiSay('Saved.', 'good'); warmed = false; }).catch(function (e) { aiSay(e.message, 'bad'); });
  });
  function aiCheck() {
    aiSay('Checking the AI. If Ollama is off, it is being started (up to 15 seconds)...');
    return api('/api/ai/check', {}).then(function (r) {
      if (!r.running) { aiSay('AI is off: Ollama is not running and could not be started. Install it from ollama.com or open the Ollama app, then press Check again. Until then simple rules read your sentences.', 'bad'); }
      else if (!r.model_installed) { aiSay('Ollama is running but this model is not installed. In a terminal run: ollama pull ' + $('#ai-model').value, 'bad'); }
      else { aiSay('AI is ready. Ollama is running and the model is installed.', 'good'); }
    }).catch(function (e) { aiSay(e.message, 'bad'); });
  }
  $('#ai-check').addEventListener('click', function () {
    aiSave().then(aiCheck).catch(function (e) { aiSay(e.message, 'bad'); });
  });
  // ---- keyboard shortcuts ----
  var gPending = false, gTimer;
  document.addEventListener('keydown', function (e) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && (e.key === 'k' || e.key === 'K')) { e.preventDefault(); if (ck.hidden) { openCk(); } else { closeCk(); } return; }
    if (e.ctrlKey || e.metaKey || e.altKey || !ck.hidden) { return; }
    var t = e.target, tag = t && t.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || (t && t.isContentEditable)) { return; }
    if ($$('dialog[open]').length) { return; }
    var k = e.key;
    if (gPending) {
      gPending = false; clearTimeout(gTimer);
      var kk = k.toLowerCase();
      if (kk === 'b') { e.preventDefault(); go('/'); } else if (kk === 'c') { e.preventDefault(); go('/calendar'); } else if (kk === 's') { e.preventDefault(); go('/summary'); }
      return;
    }
    if (k === 'g' || k === 'G') { gPending = true; gTimer = setTimeout(function () { gPending = false; }, 1500); }
    else if (k === 'c' || k === 'C') { e.preventDefault(); openCk('capture'); }
    else if (k === '/') { e.preventDefault(); var q = $('#q'); if (q) { q.focus(); } else { openCk(); } }
    else if (k === 'e' || k === 'E') { var id = selectedId(); if (id) { e.preventDefault(); openPanel(id); } }
    else if (k === '?') { e.preventDefault(); openDlg($('#help-dialog')); }
  });

  // ---- calendar: open blocks, month day clicks, real-mouse drag and resize ----
  var calClick = 0;
  var PD = '.pdrag[data-id],.un-item[data-id],.ag-item[data-id]';
  document.addEventListener('click', function (e) {
    if (Date.now() < calClick) { e.preventDefault(); e.stopPropagation(); return; }
    var p = e.target.closest && e.target.closest(PD);
    if (p) { openPanel(p.dataset.id); return; }
    var m = e.target.closest && e.target.closest('.mcell');
    if (m && !e.target.closest('a')) { go('/calendar?view=day&date=' + m.dataset.date); }
  }, true);
  document.addEventListener('keydown', function (e) {
    var p = e.target.closest && e.target.closest(PD);
    if (p && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); openPanel(p.dataset.id); }
  });
  function hh() { return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--hh')) || 48; }
  function hm(min) { return pad(Math.floor(min / 60)) + ':' + pad(min % 60); }
  var drag = null;
  document.addEventListener('pointerdown', function (e) {
    if (e.button !== 0 || e.pointerType === 'touch') { return; }
    var rz = e.target.closest && e.target.closest('.rz');
    var item = e.target.closest && e.target.closest(PD);
    if (!item || (e.target.closest('button') && !rz)) { return; }
    var r = item.getBoundingClientRect();
    drag = { el: item, id: parseInt(item.dataset.id, 10), resize: !!rz, x0: e.clientX, y0: e.clientY, moved: false, grab: e.clientY - r.top, dur: parseInt(item.dataset.dur || '30', 10), h0: r.height };
  });
  document.addEventListener('pointermove', function (e) {
    if (!drag) { return; }
    var dx = e.clientX - drag.x0, dy = e.clientY - drag.y0;
    if (!drag.moved && Math.abs(dx) + Math.abs(dy) < 5) { return; }
    if (!drag.moved) { drag.moved = true; document.body.classList.add('cal-dragging'); drag.el.classList.add('lifting'); if (!drag.resize) { drag.el.style.pointerEvents = 'none'; } }
    if (drag.resize) {
      var nd = Math.max(15, Math.round((drag.dur + dy / hh() * 60) / 15) * 15);
      drag.newDur = nd;
      drag.el.style.height = 'calc(var(--hh)*' + (nd / 60) + ' - 2px)';
      drag.el.setAttribute('data-len', nd + ' min');
    } else {
      drag.el.style.transform = 'translate(' + dx + 'px,' + dy + 'px)';
      $$('.dropt').forEach(function (x) { x.classList.remove('dropt'); });
      var u = document.elementFromPoint(e.clientX, e.clientY), t = u && u.closest && u.closest('.dcol,.mcell,.adcell');
      if (t) { t.classList.add('dropt'); }
    }
  });
  document.addEventListener('pointerup', function (e) {
    if (!drag) { return; }
    var d = drag; drag = null;
    document.body.classList.remove('cal-dragging');
    $$('.dropt').forEach(function (x) { x.classList.remove('dropt'); });
    if (!d.moved) { return; }
    calClick = Date.now() + 400;
    if (d.resize) {
      if (d.newDur && d.newDur !== d.dur) { api('/api/cards/' + d.id + '/resize', { dur_min: d.newDur }).then(handle).catch(fail); } else { refresh(); }
      return;
    }
    d.el.style.pointerEvents = ''; d.el.style.transform = '';
    var u = document.elementFromPoint(e.clientX, e.clientY);
    var col = u && u.closest && u.closest('.dcol');
    if (col) {
      var rect = col.getBoundingClientRect();
      var isBlock = d.el.classList.contains('evb');
      var mins = (e.clientY - rect.top - (isBlock ? d.grab : 0)) / hh() * 60;
      mins = Math.max(0, Math.min(1425, Math.round(mins / 15) * 15));
      api('/api/move', { moves: [{ id: d.id, to: col.dataset.date + 'T' + hm(mins) }] }).then(handle).catch(fail);
      return;
    }
    var cell = u && u.closest && u.closest('.mcell,.adcell');
    if (cell) {
      var h = d.el.dataset.hm;
      api('/api/move', { moves: [{ id: d.id, to: cell.dataset.date + 'T' + (h || '00:00'), allday: !h }] }).then(handle).catch(fail);
      return;
    }
    refresh();
  });
  document.addEventListener('pointercancel', function () { if (drag) { drag = null; document.body.classList.remove('cal-dragging'); refresh(); } });

  function initCal() {
    var sc = $('#tg-scroll'); if (!sc) { return; }
    var n = $('.nowline'), top = 7;
    if (n) { top = Math.max(0, new Date().getHours() - 2); }
    // never hide the first block of the day: start one hour before the earliest block
    $$('.evb').forEach(function (b) { top = Math.min(top, Math.max(0, Math.floor((+b.dataset.start || 0) / 60) - 1)); });
    sc.scrollTop = top * hh();
  }
  function tickNow() {
    var n = new Date(), min = n.getHours() * 60 + n.getMinutes();
    $$('.nowline').forEach(function (x) { x.style.top = 'calc(var(--hh)*' + (min / 60) + ')'; });
  }
  setInterval(tickNow, 60000);
  initCal();
  window.__initCal = function () { var sc = $('#tg-scroll'); if (sc && !sc.scrollTop) { initCal(); } };
  applyFilter();
})();
