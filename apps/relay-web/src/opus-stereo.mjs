/** Chrome only decodes Opus as stereo when the answer fmtp has stereo=1. */
export function forceOpusStereo(sdp) {
  const text = String(sdp || "");
  const nl = text.includes("\r\n") ? "\r\n" : "\n";
  const lines = text.split(/\r?\n/);
  const opus = {};
  for (let i = 0; i < lines.length; i++) {
    const rtp = lines[i].match(/^a=rtpmap:(\d+) opus\//i);
    if (rtp) opus[rtp[1]] = { rtpmap: i, fmtp: -1 };
  }
  for (let i = 0; i < lines.length; i++) {
    const fmtp = lines[i].match(/^a=fmtp:(\d+)\s+(.*)$/);
    if (!fmtp || !opus[fmtp[1]]) continue;
    opus[fmtp[1]].fmtp = i;
    let params = fmtp[2];
    params = /stereo=/i.test(params)
      ? params.replace(/stereo=\d+/gi, "stereo=1")
      : `${params};stereo=1`;
    params = /sprop-stereo=/i.test(params)
      ? params.replace(/sprop-stereo=\d+/gi, "sprop-stereo=1")
      : `${params};sprop-stereo=1`;
    lines[i] = `a=fmtp:${fmtp[1]} ${params}`;
  }
  const missing = [];
  for (const pt of Object.keys(opus)) {
    if (opus[pt].fmtp < 0) missing.push({ at: opus[pt].rtpmap + 1, pt });
  }
  missing.sort((a, b) => b.at - a.at);
  for (const row of missing) {
    lines.splice(
      row.at,
      0,
      `a=fmtp:${row.pt} minptime=10;useinbandfec=1;stereo=1;sprop-stereo=1`,
    );
  }
  return lines.join(nl);
}
