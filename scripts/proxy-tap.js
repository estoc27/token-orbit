// Token_Orbit T1 프록시 탭 — 멀티 프로바이더.
//
// AI API 트래픽을 통과시키면서 rate limit **헤더만** 관측해 프로바이더별 파일로 기록한다.
// 요청/응답 본문은 절대 저장하지 않으며, fail-open으로 동작한다 (탭이 죽어도 기록만 멈춤).
//
// 마운트 (경로 접두어로 라우팅):
//   /openai/...    → api.openai.com    (접두어 제거 후 전달)
//   /anthropic/... → api.anthropic.com (접두어 제거 후 전달)
//   그 외          → api.anthropic.com (하위 호환 — 기존 ANTHROPIC_BASE_URL=:8377 유지)
//
// 클라이언트 설정 예:
//   ANTHROPIC_BASE_URL=http://127.0.0.1:8377            (또는 .../anthropic)
//   OPENAI_BASE_URL=http://127.0.0.1:8377/openai/v1
//
// 기록 (버킷별 누적 — 이번 응답에 없는 버킷은 직전 값 유지, 리셋 지난 버킷은 삭제):
//   ~/.token-orbit/claude-headers.json   (anthropic-ratelimit-unified-*)
//   ~/.token-orbit/openai-headers.json   (x-ratelimit-{limit,remaining,reset}-{requests,tokens})

const http = require("http");
const https = require("https");
const fs = require("fs");
const os = require("os");
const path = require("path");

const PORT = Number(process.env.TOKEN_ORBIT_PROXY_PORT || 8377);
const OUT_DIR = path.join(os.homedir(), ".token-orbit");
fs.mkdirSync(OUT_DIR, { recursive: true });

// ---------- 프로바이더 정의 ----------

const PROVIDERS = {
  anthropic: {
    host: "api.anthropic.com",
    out: path.join(OUT_DIR, "claude-headers.json"),
    // anthropic-ratelimit-unified-<bucket>-{utilization,reset} → 버킷 누적
    capture(headers, now, buckets) {
      const PREFIX = "anthropic-ratelimit-unified-";
      const fresh = {};
      for (const [k, v] of Object.entries(headers)) {
        const key = k.toLowerCase();
        if (!key.startsWith(PREFIX)) continue;
        const m = key.slice(PREFIX.length).match(/^(.+)-(utilization|reset)$/);
        if (!m) continue;
        (fresh[m[1]] ||= {})[m[2]] = v;
      }
      for (const [bucket, vals] of Object.entries(fresh)) {
        if (vals.utilization === undefined) continue;
        buckets[bucket] = {
          utilization: vals.utilization,
          reset: vals.reset ?? buckets[bucket]?.reset,
          observed_at: now,
        };
      }
      return Object.keys(fresh).length > 0;
    },
  },
  openai: {
    host: "api.openai.com",
    out: path.join(OUT_DIR, "openai-headers.json"),
    // x-ratelimit-{limit,remaining}-{requests,tokens} + reset("1s"/"6m0s" 형태)
    // 분 단위 롤링 제한이라 remaining/limit을 그대로 기록하고 파서가 %로 환산한다.
    capture(headers, now, buckets) {
      let seen = false;
      for (const kind of ["requests", "tokens"]) {
        const limit = headers[`x-ratelimit-limit-${kind}`];
        const remaining = headers[`x-ratelimit-remaining-${kind}`];
        if (limit === undefined || remaining === undefined) continue;
        buckets[kind] = {
          limit,
          remaining,
          reset_after: headers[`x-ratelimit-reset-${kind}`], // "1s", "6m0s" 등 상대 시간
          observed_at: now,
        };
        seen = true;
      }
      return seen;
    },
  },
};

function loadBuckets(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8")).buckets || {};
  } catch (_) {
    return {};
  }
}

function saveBuckets(file, buckets) {
  const tmp = file + ".tmp";
  try {
    fs.writeFileSync(tmp, JSON.stringify({ buckets }, null, 2));
    fs.renameSync(tmp, file); // 원자적 교체
  } catch (_) {
    /* 기록 실패가 트래픽을 막아서는 안 된다 */
  }
}

function record(provider, headers) {
  const now = Math.floor(Date.now() / 1000);
  const buckets = loadBuckets(provider.out);
  if (!provider.capture(headers, now, buckets)) return;
  // 절대 리셋 시각(epoch)이 지난 버킷은 창이 굴러간 것 — 낡은 수치를 남기지 않는다.
  for (const [k, b] of Object.entries(buckets)) {
    if (b.reset && Number(b.reset) < now) delete buckets[k];
  }
  saveBuckets(provider.out, buckets);
}

// ---------- 프록시 본체 ----------

function route(url) {
  for (const [name, p] of Object.entries(PROVIDERS)) {
    const prefix = `/${name}`;
    if (url === prefix || url.startsWith(prefix + "/")) {
      return { provider: p, path: url.slice(prefix.length) || "/" };
    }
  }
  return { provider: PROVIDERS.anthropic, path: url }; // 하위 호환
}

const server = http.createServer((req, res) => {
  const { provider, path: upstreamPath } = route(req.url);
  const chunks = [];
  req.on("data", (c) => chunks.push(c));
  req.on("end", () => {
    const headers = { ...req.headers, host: provider.host };
    const upstream = https.request(
      { hostname: provider.host, path: upstreamPath, method: req.method, headers },
      (ur) => {
        try { record(provider, ur.headers); } catch (_) {}
        res.writeHead(ur.statusCode, ur.headers);
        ur.pipe(res); // 스트리밍 그대로 통과 — 버퍼링 없음
      }
    );
    upstream.on("error", () => {
      if (!res.headersSent) res.writeHead(502);
      res.end();
    });
    upstream.end(Buffer.concat(chunks));
  });
});

server.on("clientError", (_e, socket) => socket.destroy());
server.listen(PORT, "127.0.0.1", () => {
  console.log(`Token_Orbit proxy tap: 127.0.0.1:${PORT}`);
  for (const [name, p] of Object.entries(PROVIDERS)) {
    console.log(`  /${name} -> ${p.host}  (${path.basename(p.out)})`);
  }
});
