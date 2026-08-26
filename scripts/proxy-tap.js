// Token_Orbit T1 프록시 탭.
//
// Claude Code의 API 트래픽을 통과시키면서 rate limit 헤더만 관측해
// ~/.token-orbit/claude-headers.json 에 기록한다.
//
// 사용:
//   node proxy-tap.js            (기본 포트 8377)
//   set ANTHROPIC_BASE_URL=http://127.0.0.1:8377   ← Claude Code 쪽에 설정
//
// statusline 탭 대비 장점 (2026-08-26 실측):
//   - 모델 전용 주간 버킷(7d_oi = Fable)까지 나온다. statusline엔 없음.
//   - 세션 캐시가 아니라 **계정 실시간 상태**. idle 세션 캐시 문제가 없음.
//   - 소수점 정밀도 (0.46 vs 45).
//
// 원칙: 요청/응답 **본문은 절대 저장하지 않는다.** 헤더만 읽고 그대로 흘려보낸다.
// 프록시가 죽어도 사용자의 AI 작업이 죽지 않도록 fail-open으로 동작한다.

const http = require("http");
const https = require("https");
const fs = require("fs");
const os = require("os");
const path = require("path");

const PORT = Number(process.env.TOKEN_ORBIT_PROXY_PORT || 8377);
const TARGET = "api.anthropic.com";
const OUT_DIR = path.join(os.homedir(), ".token-orbit");
const OUT_FILE = path.join(OUT_DIR, "claude-headers.json");

fs.mkdirSync(OUT_DIR, { recursive: true });

const PREFIX = "anthropic-ratelimit-unified-";

function readExisting() {
  try {
    return JSON.parse(fs.readFileSync(OUT_FILE, "utf8")).buckets || {};
  } catch (_) {
    return {};
  }
}

// 버킷별로 **누적** 기록한다.
//
// 모델 전용 버킷(7d_oi = Fable)은 그 모델로 요청할 때만 헤더에 실린다.
// 매번 통째로 덮어쓰면 다른 모델을 쓰는 순간 Fable 창이 사라진다.
// 그래서 이번 응답에 없는 버킷은 **직전 값을 유지**하고, 각자 관측 시각을 들고 간다.
// (리셋 시각이 지난 값은 창이 굴러갔다는 뜻이므로 버린다 — 낡은 수치를 남기면 거짓말이 된다)
function capture(headers) {
  const now = Math.floor(Date.now() / 1000);
  const fresh = {};
  for (const [k, v] of Object.entries(headers)) {
    const key = k.toLowerCase();
    if (!key.startsWith(PREFIX)) continue;
    const rest = key.slice(PREFIX.length);
    const m = rest.match(/^(.+)-(utilization|reset)$/);
    if (!m) continue;
    const [, bucket, field] = m;
    (fresh[bucket] ||= {})[field] = v;
  }
  if (Object.keys(fresh).length === 0) return;

  const buckets = readExisting();
  for (const [bucket, vals] of Object.entries(fresh)) {
    if (vals.utilization === undefined) continue;
    buckets[bucket] = {
      utilization: vals.utilization,
      reset: vals.reset ?? buckets[bucket]?.reset,
      observed_at: now,
    };
  }
  // 리셋이 지난 버킷은 제거
  for (const [bucket, b] of Object.entries(buckets)) {
    if (b.reset && Number(b.reset) < now) delete buckets[bucket];
  }

  const tmp = OUT_FILE + ".tmp";
  try {
    fs.writeFileSync(tmp, JSON.stringify({ buckets }, null, 2));
    fs.renameSync(tmp, OUT_FILE); // 원자적 교체 — 절반 쓰인 파일을 읽히지 않게
  } catch (_) {
    /* 기록 실패가 트래픽을 막아서는 안 된다 */
  }
}

const server = http.createServer((req, res) => {
  const chunks = [];
  req.on("data", (c) => chunks.push(c));
  req.on("end", () => {
    const headers = { ...req.headers, host: TARGET };
    const upstream = https.request(
      { hostname: TARGET, path: req.url, method: req.method, headers },
      (ur) => {
        try { capture(ur.headers); } catch (_) {}
        res.writeHead(ur.statusCode, ur.headers);
        ur.pipe(res); // 스트리밍 그대로 통과 — 버퍼링하지 않는다
      }
    );
    upstream.on("error", () => {
      // fail-open: 업스트림 오류는 그대로 전달하고 프록시는 계속 산다.
      if (!res.headersSent) res.writeHead(502);
      res.end();
    });
    upstream.end(Buffer.concat(chunks));
  });
});

server.on("clientError", (_e, socket) => socket.destroy());
server.listen(PORT, "127.0.0.1", () => {
  console.log(`Token_Orbit proxy tap: 127.0.0.1:${PORT} -> ${TARGET}`);
  console.log(`writing: ${OUT_FILE}`);
});
