/* aurora.ts — 液体玻璃折射背景(原生 WebGL1,零依赖,~4KB)
 * React 用法:
 *   const ref = useRef<HTMLCanvasElement>(null)
 *   useEffect(() => createAurora(ref.current!), [])
 *   <canvas ref={ref} className="fixed inset-0 -z-10 h-full w-full" />
 * 性能:0.66x 降采样;页面隐藏/reduced-motion 自动静态;WebGL 缺失降级渐变。
 */
export function createAurora(canvas: HTMLCanvasElement, opts: { scale?: number } = {}): () => void {
  const ctx = canvas.getContext("webgl", { antialias: false, alpha: false, powerPreference: "low-power" });
  if (!ctx) {
    canvas.style.background = "linear-gradient(135deg,#ffe3ec,#e3e6ff 50%,#dcf5ee)";
    return () => {};
  }
  const gl = ctx; // 此后为非空 const,闭包内可收窄

  const VS = "attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}";
  const FS = [
    "precision highp float;",
    "uniform vec2 u_res;uniform float u_t;uniform vec2 u_mouse;",
    "float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}",
    "float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);",
    " return mix(mix(hash(i),hash(i+vec2(1.,0.)),f.x),mix(hash(i+vec2(0.,1.)),hash(i+vec2(1.,1.)),f.x),f.y);}",
    "float fbm(vec2 p){float v=0.,a=.5;for(int i=0;i<5;i++){v+=a*noise(p);p=p*2.03+1.7;a*=.5;}return v;}",
    "vec3 pal(float t){",
    " vec3 a=vec3(.945,.95,.97),b=vec3(1.,.74,.79),c=vec3(.66,.63,.97),d=vec3(.60,.92,.85);",
    " vec3 col=mix(a,b,smoothstep(.24,.52,t));",
    " col=mix(col,c,smoothstep(.55,.78,t));",
    " col=mix(col,d,smoothstep(.80,.96,t));",
    " return col;}",
    "void main(){",
    " vec2 p=(gl_FragCoord.xy-.5*u_res)/u_res.y;",
    " float t=u_t*.10;",
    " vec2 q=vec2(fbm(p*1.35+t),fbm(p*1.35-t*.7+4.));",
    " vec2 r=vec2(fbm(p*1.7+q*1.7+vec2(t*.35,2.3)),fbm(p*1.7+q*1.7+vec2(5.2,-t*.25)));",
    " vec2 m=(u_mouse-.5*u_res)/u_res.y;",
    " vec2 dm=p-m;float md=length(dm);",
    " r+= .14*exp(-2.2*md)*smoothstep(0.,.25,md)*normalize(dm+.001);",
    " float ca=.045;",
    " float fr=fbm(p*1.55+r*1.8+r*ca);",
    " float f =fbm(p*1.55+r*1.8);",
    " float fb=fbm(p*1.55+r*1.8-r*ca);",
    " vec3 col=vec3(pal(fr).r,pal(f).g,pal(fb).b);",
    " float edge=pow(clamp(length(r-vec2(.5))*1.15,0.,1.),3.);",
    " col+=edge*.16;",
    " col=mix(col,vec3(.955,.96,.98),smoothstep(.75,1.5,length(p))*.38);",
    " col+=(hash(gl_FragCoord.xy+fract(u_t))-.5)*.022;",
    " gl_FragColor=vec4(col,1.);",
    "}",
  ].join("\n");

  function sh(type: number, src: string): WebGLShader {
    const s = gl.createShader(type)!;
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) console.error("[aurora]", gl.getShaderInfoLog(s));
    return s;
  }
  const prog = gl.createProgram()!;
  gl.attachShader(prog, sh(gl.VERTEX_SHADER, VS));
  gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FS));
  gl.linkProgram(prog);
  gl.useProgram(prog);

  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "p");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  const uRes = gl.getUniformLocation(prog, "u_res");
  const uT = gl.getUniformLocation(prog, "u_t");
  const uM = gl.getUniformLocation(prog, "u_mouse");

  const scale = opts.scale || 0.66;
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  let running = true;
  let raf = 0;
  let tx = -1e4, ty = -1e4; // 指针目标位置
  let mx = -1e4, my = -1e4; // 阻尼跟随后的实际位置

  function resize() {
    canvas.width = Math.max(2, Math.floor(canvas.clientWidth * scale));
    canvas.height = Math.max(2, Math.floor(canvas.clientHeight * scale));
    gl.viewport(0, 0, canvas.width, canvas.height);
  }
  function onMove(e: PointerEvent) {
    const r = canvas.getBoundingClientRect();
    if (r.width < 2) return;
    tx = ((e.clientX - r.left) / r.width) * canvas.width;
    ty = (1 - (e.clientY - r.top) / r.height) * canvas.height;
    if (mx < -1e3) { mx = tx; my = ty; } // 首次直接落位,避免从远处飞入
  }
  function onVis() {
    running = !document.hidden && !still;
    if (running) raf = requestAnimationFrame(loop);
  }
  window.addEventListener("resize", resize);
  window.addEventListener("pointermove", onMove, { passive: true });
  document.addEventListener("visibilitychange", onVis);
  resize();

  const start = performance.now();
  function draw(now: number) {
    mx += (tx - mx) * 0.055; // 阻尼跟随:搅动柔和的关键
    my += (ty - my) * 0.055;
    gl.uniform2f(uRes, canvas.width, canvas.height);
    gl.uniform1f(uT, (now - start) / 1000);
    gl.uniform2f(uM, mx, my);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }
  function loop(now: number) {
    if (!running) return;
    draw(now);
    raf = requestAnimationFrame(loop);
  }
  if (still) draw(start + 3000); // reduced-motion:单帧静态
  else raf = requestAnimationFrame(loop);

  return () => {
    running = false;
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", resize);
    window.removeEventListener("pointermove", onMove);
    document.removeEventListener("visibilitychange", onVis);
    gl.getExtension("WEBGL_lose_context")?.loseContext();
  };
}
