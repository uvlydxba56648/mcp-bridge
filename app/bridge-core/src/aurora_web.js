/* aurora.js — 液体玻璃折射背景(原生 WebGL1,零依赖,~4KB)
 * 用法:const stop = createAurora(canvas); 停止:stop()
 * 性能:0.66x 降采样渲染;页面隐藏自动暂停;shader 编译 <50ms(真实 GPU)
 */
function createAurora(canvas, opts) {
  opts = opts || {};
  var gl = canvas.getContext("webgl", { antialias: false, alpha: false, powerPreference: "low-power" });
  if (!gl) { canvas.style.background = "linear-gradient(135deg,#ffe3ec,#e3e6ff 50%,#dcf5ee)"; return function () {}; }

  var VS = "attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}";
  var FS = [
    "precision highp float;",
    "uniform vec2 u_res;uniform float u_t;uniform vec2 u_mouse;",
    "float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}",
    "float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);",
    " return mix(mix(hash(i),hash(i+vec2(1.,0.)),f.x),mix(hash(i+vec2(0.,1.)),hash(i+vec2(1.,1.)),f.x),f.y);}",
    "float fbm(vec2 p){float v=0.,a=.5;for(int i=0;i<5;i++){v+=a*noise(p);p=p*2.03+1.7;a*=.5;}return v;}",
    /* 粉彩极光调色板:近白底 → 粉 → 紫 → 青 */
    "vec3 pal(float t){",
    " vec3 a=vec3(.945,.95,.97),b=vec3(1.,.74,.79),c=vec3(.66,.63,.97),d=vec3(.60,.92,.85);",
    " vec3 col=mix(a,b,smoothstep(.24,.52,t));",
    " col=mix(col,c,smoothstep(.55,.78,t));",
    " col=mix(col,d,smoothstep(.80,.96,t));",
    " return col;}",
    "void main(){",
    " vec2 p=(gl_FragCoord.xy-.5*u_res)/u_res.y;",
    " float t=u_t*.10;",,
    /* 域扭曲:液体流动的核心 */
    " vec2 q=vec2(fbm(p*1.35+t),fbm(p*1.35-t*.7+4.));",
    " vec2 r=vec2(fbm(p*1.7+q*1.7+vec2(t*.35,2.3)),fbm(p*1.7+q*1.7+vec2(5.2,-t*.25)));",
    /* 鼠标引力:折射中心跟随指针 */
    " vec2 m=(u_mouse-.5*u_res)/u_res.y;",
    " vec2 dm=p-m;float md=length(dm);",
    " r+= .14*exp(-2.2*md)*smoothstep(0.,.25,md)*normalize(dm+.001);",,
    /* 色散:RGB 三通道沿扭曲梯度微量错位采样 = 棱镜折射感 */
    " float ca=.045;",
    " float fr=fbm(p*1.55+r*1.8+r*ca);",
    " float f =fbm(p*1.55+r*1.8);",
    " float fb=fbm(p*1.55+r*1.8-r*ca);",
    " vec3 col=vec3(pal(fr).r,pal(f).g,pal(fb).b);",
    /* 高光棱线:扭曲变化剧烈处提亮,像玻璃边缘 */
    " float edge=pow(clamp(length(r-vec2(.5))*1.15,0.,1.),3.);",
    " col+=edge*.16;",
    /* 四角轻微泛白,中心聚焦内容 */
    " col=mix(col,vec3(.955,.96,.98),smoothstep(.75,1.5,length(p))*.38);",
    /* 细噪点,去色带 */
    " col+=(hash(gl_FragCoord.xy+fract(u_t))-.5)*.022;",
    " gl_FragColor=vec4(col,1.);",
    "}"
  ].join("\n");

  function sh(type, src) {
    var s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) { console.error(gl.getShaderInfoLog(s)); return null; }
    return s;
  }
  var t0 = performance.now();
  var prog = gl.createProgram();
  gl.attachShader(prog, sh(gl.VERTEX_SHADER, VS));
  gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FS));
  gl.linkProgram(prog); gl.useProgram(prog);
  console.log("[aurora] shader compile+link: " + (performance.now() - t0).toFixed(1) + "ms");

  var buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  var loc = gl.getAttribLocation(prog, "p");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  var uRes = gl.getUniformLocation(prog, "u_res"),
      uT = gl.getUniformLocation(prog, "u_t"),
      uM = gl.getUniformLocation(prog, "u_mouse");

  var scale = opts.scale || 0.66, running = true, raf = 0, mx = -1e4, my = -1e4, tx = -1e4, ty = -1e4;
  function resize() {
    canvas.width = Math.max(2, Math.floor(canvas.clientWidth * scale));
    canvas.height = Math.max(2, Math.floor(canvas.clientHeight * scale));
    gl.viewport(0, 0, canvas.width, canvas.height);
  }
  window.addEventListener("resize", resize);
  window.addEventListener("pointermove", function (e) {
    var r = canvas.getBoundingClientRect();
    if (r.width < 2) return;
    tx = ((e.clientX - r.left) / r.width) * canvas.width;
    ty = (1 - (e.clientY - r.top) / r.height) * canvas.height;
    if (mx < -1e3) { mx = tx; my = ty; }
  });
  document.addEventListener("visibilitychange", function () {
    running = !document.hidden;
    if (running) loop(performance.now());
  });
  resize();
  var start = performance.now(), first = true;
  function loop(now) {
    if (!running) return;
    mx += (tx - mx) * 0.055;
    my += (ty - my) * 0.055;
    gl.uniform2f(uRes, canvas.width, canvas.height);
    gl.uniform1f(uT, (now - start) / 1000);
    gl.uniform2f(uM, mx, my);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    if (first) { first = false; console.log("[aurora] first frame: " + (performance.now() - start).toFixed(1) + "ms"); }
    raf = requestAnimationFrame(loop);
  }
  loop(performance.now());
  return function () { running = false; cancelAnimationFrame(raf); };
}
