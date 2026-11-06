"""Summarise a media prototype measurement log (JSON lines) per trial phase."""
import json
import statistics
import sys

def mean(values):
    values = [v for v in values if isinstance(v, (int, float))]
    return round(statistics.fmean(values), 1) if values else None

def number(value):
    try:
        return float(value)
    except (TypeError, ValueError):
        return None

def parse(lines):
    for line in lines:
        try:
            yield json.loads(line)
        except json.JSONDecodeError:
            continue  # an unfinished last line while the session still runs

events = list(parse(open(sys.argv[1], encoding='utf-8')))
starts = [e for e in events if e['type'] == 'trial']
end = next((e['t_ms'] for e in events if e['type'] == 'trial_end'), events[-1]['t_ms'])
phases = []
for index, start in enumerate(starts):
    stop = starts[index + 1]['t_ms'] if index + 1 < len(starts) else end
    inside = [e for e in events if start['t_ms'] <= e['t_ms'] < stop]
    stats = [e for e in inside if e['type'] == 'stats']
    clients = [e['stats'] for e in inside if e['type'] == 'client']
    decisions = [{'after_ms': e['t_ms'] - start['t_ms'], 'reason': e['reason'], 'kbps': e['params']['bitrate_bps'] // 1000,
                  'scale': e['params']['scale'], 'fps': e['params']['fps']} for e in inside if e['type'] == 'decision']
    targets = [s['target']['bitrate_bps'] / 1000 for s in stats if s.get('target')]
    first, last = (stats[0], stats[-1]) if stats else ({}, {})
    phases.append({
        'phase': index,
        'profile': start['profile'],
        'seconds': round((stop - start['t_ms']) / 1000, 1),
        'decisions': decisions,
        'first_decision_ms': decisions[0]['after_ms'] if decisions else None,
        'target_kbps_start_end_min': [targets[0] if targets else None, targets[-1] if targets else None, min(targets) if targets else None],
        'estimate_kbps_mean': mean([s['estimate_bps'] / 1000 for s in stats if s.get('estimate_bps')]),
        'sent_kbps_mean': mean([s['sent_bps'] / 1000 for s in stats]),
        'queue_ms_mean_max': [mean([s['queue_ms'] for s in stats]), max((s['queue_ms'] for s in stats), default=None)],
        'encode_ms_mean': mean([s['encode_ms'] for s in stats]),
        'encoded_sizes': sorted({f"{s['width']}x{s['height']}" for s in stats}),
        'emulator_drops_loss_queue': [last.get('dropped_loss', 0) - first.get('dropped_loss', 0), last.get('dropped_queue', 0) - first.get('dropped_queue', 0)],
        'client_fps_mean': mean([number(c.get('client fps')) for c in clients]),
        'client_kbps_mean': mean([number(c.get('client kbps')) for c in clients]),
        'client_sizes': sorted({c.get('client size') for c in clients if c.get('client size')}),
        'client_jitter_ms_mean': mean([number(c.get('jitter ms')) for c in clients]),
        'client_jitter_buffer_ms_mean': mean([number(c.get('jitter buffer ms')) for c in clients]),
        'client_rtt_ms_mean': mean([number(c.get('rtt ms')) for c in clients]),
        'client_freezes_delta': (number(clients[-1].get('freezes')) or 0) - (number(clients[0].get('freezes')) or 0) if clients else None,
        'client_packets_lost_delta': (number(clients[-1].get('packets lost')) or 0) - (number(clients[0].get('packets lost')) or 0) if clients else None,
        'source': first.get('source'),
    })
print(json.dumps(phases, indent=1, ensure_ascii=False))
