// Greenmote's hero: the program's icon grown in 3D over a meadow at night, rendered with three.js.
//
// The icon is a crescent moon made of grass, a gnarled tree in its hollow and three strings of
// beads hanging beneath it like a dreamcatcher's, ending in an oak leaf, a maple leaf and a slim
// leaf. Here the crescent is a rounded body furred with combed blades, the tree has a bark trunk,
// roots on the inner rim and a canopy of leaf cards, and the strings are verlet chains of beads
// that the wind and the pointer swing, their leaves fluttering at the ends. Pale specks drift
// through the hollow as they do on the icon.
//
// Around it lies a field of instanced grass rolling to the horizon, bending under gusts that travel
// across it, with mist in the low ground and small trees silhouetted at several depths. Above is a
// night sky of stars and a faint aurora, and fireflies drift over the grass. The pointer is a lamp
// that pushes the grass and the beads; a click sends a gust out across the field and swings the
// strings.
//
// The scene renders to a half-float target; a bright pass and four blur passes make the bloom, and
// the composite applies ACES tone mapping, a scrim behind the hero's text and dithering. The aurora
// is marched at half resolution into its own target first. Colours
// come from the site's CSS tokens. The mark stands beside the hero's text, measured at every
// layout, or above it on a phone, where sass/brand.sass leaves it room. Nothing runs until the hero
// is on screen or while the tab is hidden, and the resolution and the grass thin out if frames run
// slow. Under prefers-reduced-motion one frame is drawn. Until the first frame, and without WebGL, a
// still of the mark stands in its place.

import * as THREE from './vendor/three.module.min.js';

// The crescent: the outer circle has radius 1. The disc of the inner circle, 0.412 from the centre
// along an axis 21 degrees above the right, with radius 0.692, is cut away, so the horns end at 54
// and -12 degrees, where the icon's do, and the back is 0.72 thick.
const CRESCENT = { axis: THREE.MathUtils.degToRad(21), offset: 0.412, inner: 0.692, depth: 0.3, back: 0.72 };
const INNER = new THREE.Vector2(Math.cos(CRESCENT.axis) * CRESCENT.offset, Math.sin(CRESCENT.axis) * CRESCENT.offset);
const HORN = Math.acos((1 + CRESCENT.offset ** 2 - CRESCENT.inner ** 2) / (2 * CRESCENT.offset));
const PHI_START = CRESCENT.axis + HORN;
const PHI_END = CRESCENT.axis + Math.PI * 2 - HORN;
// The mark, strings and leaves included, spans x from -1.02 to 1.05 and y from -1.66 to 1.
export const MARK = { center: new THREE.Vector2(0.015, -0.33), width: 2.1, height: 2.66 };
const MARK_ASPECT = MARK.width / MARK.height;
// The still has this much room round the mark, as a fraction of the mark's height, for the fur.
const STILL_MARGIN = 0.05;
// The strings hang from the crescent's underside, as on the icon: outer-rim angles in degrees.
const STRINGS = [
  { angle: 214, nodes: 8, leaf: 'oak' },
  { angle: 268, nodes: 5, leaf: 'maple' },
  { angle: 322, nodes: 9, leaf: 'slim' },
];
const LINK = 0.052;
const CAMERA = { y: 1.45, z: 8, fov: 34, pitch: THREE.MathUtils.degToRad(6.5) };
const MARK_Z = 1.2;
const FIELD = { near: 6, far: 72 };
const WIND = new THREE.Vector2(0.82, 0.57).normalize();

const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;

function cssColor(name, fallback) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const color = new THREE.Color(fallback);
  if (raw) {
    try { color.setStyle(raw); } catch { /* an unparsable token keeps the fallback */ }
  }
  return color;
}

function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// The ground: a gentle swell near by, hills rising with distance. It never changes, so it is worked
// out here once, for the terrain, the field's blades and the trees on the hills, rather than by
// every vertex of them every frame. This is the same noise the shaders use.
const fract = (x) => x - Math.floor(x);
function hash12(x, y) {
  let a = fract(x * 0.1031);
  let b = fract(y * 0.1031);
  let c = a;
  const d = a * (b + 33.33) + b * (c + 33.33) + c * (a + 33.33);
  a += d;
  b += d;
  c += d;
  return fract((a + b) * c);
}
function vnoise(x, y) {
  const ix = Math.floor(x);
  const iy = Math.floor(y);
  const fx = x - ix;
  const fy = y - iy;
  const ux = fx * fx * (3 - 2 * fx);
  const uy = fy * fy * (3 - 2 * fy);
  const bottom = hash12(ix, iy) + (hash12(ix + 1, iy) - hash12(ix, iy)) * ux;
  const top = hash12(ix, iy + 1) + (hash12(ix + 1, iy + 1) - hash12(ix, iy + 1)) * ux;
  return bottom + (top - bottom) * uy;
}
function fbm(x, y) {
  let sum = 0;
  let amplitude = 0.5;
  for (let i = 0; i < 4; i++) {
    sum += amplitude * vnoise(x, y);
    x = x * 2.03 + 17.1;
    y = y * 2.03 + 9.7;
    amplitude *= 0.5;
  }
  return sum;
}
function smoothstep(edge0, edge1, x) {
  const t = Math.min(1, Math.max(0, (x - edge0) / (edge1 - edge0)));
  return t * t * (3 - 2 * t);
}
function groundHeight(x, z) {
  const ahead = Math.max(0, CAMERA.z - z);
  const swell = (fbm(x * 0.07 + 3, z * 0.07 + 1) - 0.45) * 0.9;
  const hills = (fbm(x * 0.018 - 4, z * 0.026 + 7) - 0.3) * 11;
  return swell + hills * smoothstep(14, 80, ahead);
}

// Texture baking ---------------------------------------------------------------------------------

function canvas2d(width, height) {
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  return [canvas, canvas.getContext('2d')];
}

function texture(canvas, colorSpace, repeat) {
  const map = new THREE.CanvasTexture(canvas);
  map.colorSpace = colorSpace;
  map.anisotropy = 4;
  if (repeat) map.wrapS = map.wrapT = THREE.RepeatWrapping;
  map.needsUpdate = true;
  return map;
}

// A normal map from a canvas's brightness, read as height.
function normalFrom(canvas, strength, wrap) {
  const { width, height } = canvas;
  const source = canvas.getContext('2d').getImageData(0, 0, width, height).data;
  const [out, context] = canvas2d(width, height);
  const image = context.createImageData(width, height);
  const lum = (x, y) => {
    const xx = wrap ? (x + width) % width : Math.min(width - 1, Math.max(0, x));
    const yy = (y + height) % height;
    const i = (yy * width + xx) * 4;
    return (source[i] * 0.3 + source[i + 1] * 0.59 + source[i + 2] * 0.11) / 255;
  };
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const dx = (lum(x + 1, y) - lum(x - 1, y)) * strength;
      const dy = (lum(x, y + 1) - lum(x, y - 1)) * strength;
      const length = Math.hypot(dx, dy, 1);
      const i = (y * width + x) * 4;
      image.data[i] = (-dx / length * 0.5 + 0.5) * 255;
      image.data[i + 1] = (dy / length * 0.5 + 0.5) * 255;
      image.data[i + 2] = (1 / length * 0.5 + 0.5) * 255;
      image.data[i + 3] = 255;
    }
  }
  context.putImageData(image, 0, 0);
  return texture(out, THREE.NoColorSpace, wrap);
}

// The crescent's hide: dense strands of grass running along it and slanting across, as the icon
// draws them, over a dark green ground.
function strandMaps(small) {
  const width = small ? 1024 : 2048;
  const height = small ? 128 : 256;
  const [canvas, context] = canvas2d(width, height);
  const next = random(17);
  context.fillStyle = '#0a200d';
  context.fillRect(0, 0, width, height);
  context.lineCap = 'round';
  const strokes = small ? 2600 : 5200;
  for (let i = 0; i < strokes; i++) {
    const x = next() * width;
    const y = next() * height;
    const length = (30 + next() * 120) * (width / 2048);
    const slant = -0.5 + next() * 0.35;
    const light = 10 + next() * 34;
    context.strokeStyle = `hsla(${104 + next() * 26}, ${45 + next() * 30}%, ${light}%, ${0.35 + next() * 0.5})`;
    context.lineWidth = 0.6 + next() * 1.8;
    for (const offset of [-height, 0, height]) {
      context.beginPath();
      context.moveTo(x, y + offset);
      context.quadraticCurveTo(x + length * 0.5, y + offset + length * slant * 0.3 + (next() - 0.5) * 6, x + length, y + offset + length * slant);
      context.stroke();
    }
  }
  return { map: texture(canvas, THREE.SRGBColorSpace, true), normal: normalFrom(canvas, 5, false) };
}

// Bark: reddish brown with dark furrows running along the trunk.
function barkMaps() {
  const [canvas, context] = canvas2d(512, 128);
  const next = random(29);
  context.fillStyle = '#5e2618';
  context.fillRect(0, 0, 512, 128);
  for (let i = 0; i < 900; i++) {
    const y = next() * 128;
    const x = next() * 512;
    const dark = next() < 0.55;
    context.strokeStyle = dark ? `rgba(28, 10, 6, ${0.3 + next() * 0.5})` : `rgba(${150 + next() * 40}, ${70 + next() * 30}, ${40 + next() * 20}, ${0.2 + next() * 0.35})`;
    context.lineWidth = 0.8 + next() * 2.5;
    context.beginPath();
    context.moveTo(x, y);
    context.lineTo(x + 20 + next() * 70, y + (next() - 0.5) * 5);
    context.stroke();
  }
  return { map: texture(canvas, THREE.SRGBColorSpace, true), normal: normalFrom(canvas, 4, true) };
}

// A sprig of seven leaves for the canopy's cards, on a transparent ground.
function sprigTexture() {
  const size = 256;
  const [canvas, context] = canvas2d(size, size);
  const next = random(41);
  const leaf = (x, y, angle, length, width, shade) => {
    context.save();
    context.translate(x, y);
    context.rotate(angle);
    const gradient = context.createLinearGradient(0, -width, 0, width);
    gradient.addColorStop(0, `hsl(${112 + shade * 16}, 52%, ${20 + shade * 18}%)`);
    gradient.addColorStop(1, `hsl(${120 + shade * 10}, 48%, ${11 + shade * 10}%)`);
    context.fillStyle = gradient;
    context.beginPath();
    context.moveTo(0, 0);
    context.quadraticCurveTo(length * 0.45, -width, length, 0);
    context.quadraticCurveTo(length * 0.45, width, 0, 0);
    context.fill();
    context.strokeStyle = 'rgba(8, 24, 8, 0.55)';
    context.lineWidth = 1.2;
    context.beginPath();
    context.moveTo(2, 0);
    context.lineTo(length * 0.92, 0);
    context.stroke();
    context.restore();
  };
  for (let i = 0; i < 9; i++) {
    const angle = (i / 9) * Math.PI * 2 + next() * 0.5;
    const reach = 18 + next() * 40;
    leaf(128 + Math.cos(angle) * reach * 0.5, 128 + Math.sin(angle) * reach * 0.5, angle, 70 + next() * 40, 18 + next() * 10, next());
  }
  return texture(canvas, THREE.SRGBColorSpace, false);
}

// A hanging leaf's face: a green blade with a pale midrib and side veins, darker at the edges.
function veinTexture() {
  const size = 256;
  const [canvas, context] = canvas2d(size, size);
  const next = random(53);
  const gradient = context.createLinearGradient(0, 0, 0, size);
  gradient.addColorStop(0, '#3f8a34');
  gradient.addColorStop(1, '#2a6526');
  context.fillStyle = gradient;
  context.fillRect(0, 0, size, size);
  for (let i = 0; i < 1400; i++) {
    context.fillStyle = `rgba(${20 + next() * 60}, ${70 + next() * 70}, ${20 + next() * 40}, 0.18)`;
    context.fillRect(next() * size, next() * size, 2 + next() * 5, 2 + next() * 5);
  }
  context.strokeStyle = 'rgba(190, 230, 150, 0.55)';
  context.lineWidth = 3;
  context.beginPath();
  context.moveTo(size / 2, 0);
  context.lineTo(size / 2, size);
  context.stroke();
  context.lineWidth = 1.4;
  context.strokeStyle = 'rgba(170, 215, 130, 0.4)';
  for (let i = 1; i < 9; i++) {
    const y = i * size / 9;
    for (const side of [-1, 1]) {
      context.beginPath();
      context.moveTo(size / 2, y);
      context.quadraticCurveTo(size / 2 + side * 40, y + 12, size / 2 + side * 110, y + 40);
      context.stroke();
    }
  }
  const edge = context.createRadialGradient(size / 2, size / 2, size * 0.2, size / 2, size / 2, size * 0.62);
  edge.addColorStop(0, 'rgba(0, 0, 0, 0)');
  edge.addColorStop(1, 'rgba(6, 20, 6, 0.55)');
  context.fillStyle = edge;
  context.fillRect(0, 0, size, size);
  return texture(canvas, THREE.SRGBColorSpace, false);
}

// The beads: mottled moss greens, grassy to the touch.
function mossTexture() {
  const size = 128;
  const [canvas, context] = canvas2d(size, size);
  const next = random(67);
  context.fillStyle = '#17401b';
  context.fillRect(0, 0, size, size);
  for (let i = 0; i < 900; i++) {
    context.fillStyle = `hsla(${100 + next() * 40}, 50%, ${10 + next() * 26}%, 0.5)`;
    context.beginPath();
    context.arc(next() * size, next() * size, 0.8 + next() * 2.4, 0, Math.PI * 2);
    context.fill();
  }
  return texture(canvas, THREE.SRGBColorSpace, true);
}

// Shaders ----------------------------------------------------------------------------------------

const NOISE = /* glsl */ `
  float hash12(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
  }
  float vnoise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    float a = hash12(i);
    float b = hash12(i + vec2(1.0, 0.0));
    float c = hash12(i + vec2(0.0, 1.0));
    float d = hash12(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
  }
  float fbm(vec2 p) {
    float sum = 0.0;
    float amplitude = 0.5;
    for (int i = 0; i < 4; i++) {
      sum += amplitude * vnoise(p);
      p = p * 2.03 + vec2(17.1, 9.7);
      amplitude *= 0.5;
    }
    return sum;
  }
  vec3 safeNormalize(vec3 v) {
    float length2 = dot(v, v);
    return length2 > 1e-10 ? v * inversesqrt(length2) : vec3(0.0, 1.0, 0.0);
  }
`;

// The ground: a gentle swell near by, hills rising with distance. Every shader that stands
// something on the ground uses this same function.
const GROUND = /* glsl */ `
  uniform float uCameraZ;
  float groundHeight(vec2 p) {
    float ahead = max(0.0, uCameraZ - p.y);
    float swell = (fbm(p * 0.07 + vec2(3.0, 1.0)) - 0.45) * 0.9;
    float hills = (fbm(p * vec2(0.018, 0.026) + vec2(-4.0, 7.0)) - 0.3) * 11.0;
    return swell + hills * smoothstep(14.0, 80.0, ahead);
  }
`;

// Wind: gusts that travel across the field, a rolling wave within them, and a click's ring.
const WIND_GLSL = /* glsl */ `
  uniform vec2 uWind;
  uniform float uTime;
  uniform vec4 uGust;
  float windAt(vec2 p) {
    float along = dot(p, uWind);
    float across = dot(p, vec2(-uWind.y, uWind.x));
    float gust = vnoise(vec2(along * 0.09 - uTime * 0.55, across * 0.07));
    float wave = 0.5 + 0.5 * sin(along * 0.5 - uTime * 1.9 + vnoise(p * 0.06) * 5.0);
    return gust * 0.75 + wave * gust * 0.55;
  }
  vec2 gustPush(vec2 p) {
    vec2 d = p - uGust.xy;
    float r = length(d);
    float front = uGust.z * 7.0;
    float wave = exp(-(r - front) * (r - front) * 0.35) * uGust.w * exp(-uGust.z * 0.7);
    return r > 1e-3 ? d / r * wave : vec2(0.0);
  }
`;

// Light shared by everything drawn with these shaders: the sky's ambient, a cool key from in front,
// the moon behind (a rim, and light through the blades), the pointer's lamp and the hollow's glow.
const LIGHT = /* glsl */ `
  uniform vec3 uSky;
  uniform vec3 uGroundTint;
  uniform vec3 uKeyDir;
  uniform vec3 uKeyColor;
  uniform vec3 uMoonDir;
  uniform vec3 uMoonColor;
  uniform vec3 uLampPos;
  uniform vec3 uLampColor;
  uniform float uLamp;
  uniform vec3 uGlowPos;
  uniform vec3 uGlowColor;
  uniform float uGlow;
  vec3 shade(vec3 albedo, vec3 n, vec3 world, float occlusion, float thin) {
    vec3 view = safeNormalize(world - cameraPosition);
    vec3 ambient = mix(uGroundTint, uSky, n.y * 0.5 + 0.5);
    float key = max(dot(n, uKeyDir), 0.0);
    float moon = max(dot(n, uMoonDir), 0.0);
    float back = max(dot(view, uMoonDir), 0.0);
    float through = back * back;
    through *= through;
    through *= through;
    vec3 lit = albedo * (ambient + uKeyColor * key * 0.7 + uMoonColor * moon * 0.35) * occlusion;
    lit += albedo * uMoonColor * through * thin * 1.8;
    vec3 toLamp = uLampPos - world;
    lit += albedo * uLampColor * uLamp / (1.0 + dot(toLamp, toLamp) * 2.5) * (0.35 + 0.65 * max(dot(n, safeNormalize(toLamp)), 0.0));
    vec3 toGlow = uGlowPos - world;
    lit += albedo * uGlowColor * uGlow / (1.0 + dot(toGlow, toGlow) * 18.0) * (0.4 + 0.6 * max(dot(n, safeNormalize(toGlow)), 0.0));
    return lit;
  }
`;

// Distance fog towards the horizon's colour, and moonlit mist lying in the low ground.
const AIR = /* glsl */ `
  uniform vec3 uFog;
  uniform vec3 uMist;
  uniform float uFogDensity;
  vec3 atmosphere(vec3 color, vec3 world) {
    float distance = length(world - cameraPosition);
    float low = 1.0 - smoothstep(-0.9, 0.9, world.y);
    float drift = vnoise(world.xz * 0.07 + vec2(uTime * 0.035, uTime * 0.015));
    float mist = low * smoothstep(7.0, 26.0, distance) * (0.3 + 0.7 * drift) * 0.8;
    color = mix(color, uMist, clamp(mist, 0.0, 1.0));
    float fog = 1.0 - exp(-distance * uFogDensity);
    return mix(color, uFog, fog);
  }
`;

// The field's blades: each instance carries a position across the view and a depth, spaced so the
// blades are evenly dense on screen; its root, the ground's height there and how clumped the grass
// is are worked out once per layout.
const FIELD_VERTEX = /* glsl */ `
  attribute vec4 aSpot;
  attribute vec4 aRoot;
  uniform float uCameraZ;
  uniform vec4 uPush;
  varying float vT;
  varying float vShade;
  varying vec3 vWorld;
  varying vec3 vNormal;
  ${NOISE}
  ${WIND_GLSL}
  void main() {
    vec2 root = aRoot.xy;
    float ground = aRoot.z;
    float clump = aRoot.w;
    float ahead = uCameraZ - root.y;
    float seed = aSpot.z;
    float height = (0.22 + seed * 0.32 + clump * 0.28) * (1.0 + ahead * 0.012);
    float width = (0.028 + aSpot.w * 0.022) * (1.0 + ahead * 0.045);
    float t = position.y;
    float angle = aSpot.w * 43.98;
    vec2 facing = vec2(cos(angle), sin(angle));
    float wind = windAt(root);
    vec2 lean = uWind * (0.1 + wind * 0.9) + facing * 0.14;
    lean += vec2(sin(uTime * 2.3 + seed * 40.0), cos(uTime * 1.7 + seed * 23.0)) * 0.04 * (0.3 + wind);
    vec2 away = root - uPush.xy;
    float distance = length(away);
    float push = uPush.z * (1.0 - smoothstep(0.0, uPush.w, distance));
    if (distance > 1e-4) lean += away / distance * push * 1.5;
    lean += gustPush(root) * 1.6;
    float bend = clamp(length(lean), 0.02, 1.35);
    vec2 leanDir = lean / max(length(lean), 1e-4);
    float along = height * (1.0 - cos(bend * t)) / bend;
    float up = height * sin(bend * t) / bend;
    vec3 side = vec3(-facing.y, 0.0, facing.x);
    vec3 world = vec3(root.x, ground, root.y) + side * position.x * width;
    world.xz += leanDir * along;
    world.y += up;
    vec3 tangent = vec3(leanDir.x * sin(bend * t), cos(bend * t), leanDir.y * sin(bend * t));
    vec3 normal = safeNormalize(cross(side, tangent));
    vNormal = normalize(mix(normal, vec3(0.0, 1.0, 0.0), 0.35));
    vT = t;
    vShade = fract(seed * 7.13) * 0.6 + clump * 0.4;
    vWorld = world;
    gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
  }
`;

const GRASS_FRAGMENT = /* glsl */ `
  uniform vec3 uBase;
  uniform vec3 uTip;
  uniform float uTime;
  varying float vT;
  varying float vShade;
  varying vec3 vWorld;
  varying vec3 vNormal;
  ${NOISE}
  ${LIGHT}
  ${AIR}
  void main() {
    vec3 n = normalize(vNormal);
    if (!gl_FrontFacing) n = -n;
    #ifdef FIELD
    vec3 albedo = mix(uBase, uTip, vT * vT) * (0.65 + 0.7 * vShade);
    vec3 color = shade(albedo, n, vWorld, 0.22 + 0.78 * vT, vT * vT);
    color = atmosphere(color, vWorld);
    #else
    // The fur is matte: sky, key and moon as broad diffuse light and the hollow's steady glow, with
    // no light through the blades and no lamp, so nothing on it glints as it stirs.
    vec3 albedo = mix(uBase, uTip, vT) * (0.85 + 0.3 * vShade);
    vec3 ambient = mix(uGroundTint, uSky, n.y * 0.5 + 0.5);
    vec3 color = albedo * (ambient + uKeyColor * max(dot(n, uKeyDir), 0.0) * 0.8 + uMoonColor * max(dot(n, uMoonDir), 0.0) * 0.25) * (0.4 + 0.6 * vT);
    vec3 toGlow = uGlowPos - vWorld;
    color += albedo * uGlowColor * 0.6 / (1.0 + dot(toGlow, toGlow) * 18.0);
    #endif
    gl_FragColor = vec4(color, 1.0);
  }
`;

const TERRAIN_VERTEX = /* glsl */ `
  varying vec3 vWorld;
  varying vec3 vNormal;
  void main() {
    vNormal = normal;
    vWorld = position;
    gl_Position = projectionMatrix * viewMatrix * vec4(position, 1.0);
  }
`;

const TERRAIN_FRAGMENT = /* glsl */ `
  uniform vec3 uBase;
  uniform vec3 uTip;
  uniform float uTime;
  varying vec3 vWorld;
  varying vec3 vNormal;
  ${NOISE}
  ${LIGHT}
  ${AIR}
  void main() {
    vec3 n = normalize(vNormal);
    float grain = vnoise(vWorld.xz * 1.7) * 0.6 + vnoise(vWorld.xz * 0.15) * 0.4;
    vec3 albedo = mix(uBase * 0.55, uTip * 0.45, grain);
    vec3 color = shade(albedo, n, vWorld, 0.45, 0.25);
    gl_FragColor = vec4(atmosphere(color, vWorld), 1.0);
  }
`;

// Small trees on the hills: each instance places one of the baked shapes on the ground, turned and
// scaled; its crown sways in the wind.
const TREE_VERTEX = /* glsl */ `
  attribute vec4 aTree;
  attribute float aGround;
  attribute float aPart;
  varying vec3 vWorld;
  varying vec3 vNormal;
  varying float vPart;
  ${NOISE}
  ${WIND_GLSL}
  void main() {
    vec2 root = aTree.xy;
    float c = cos(aTree.w);
    float s = sin(aTree.w);
    vec3 p = position * aTree.z;
    p.xz = mat2(c, -s, s, c) * p.xz;
    vec3 n = normal;
    n.xz = mat2(c, -s, s, c) * n.xz;
    float sway = aPart * position.y * (0.03 + windAt(root) * 0.05) * aTree.z;
    p.xz += uWind * sway + vec2(sin(uTime * 1.1 + root.x), cos(uTime * 0.9 + root.y)) * aPart * position.y * 0.012 * aTree.z;
    vec3 world = vec3(root.x, aGround - 0.12 * aTree.z, root.y) + p;
    vWorld = world;
    vNormal = n;
    vPart = aPart;
    gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
  }
`;

const TREE_FRAGMENT = /* glsl */ `
  uniform vec3 uBark;
  uniform vec3 uCrown;
  uniform float uTime;
  varying vec3 vWorld;
  varying vec3 vNormal;
  varying float vPart;
  ${NOISE}
  ${LIGHT}
  ${AIR}
  void main() {
    vec3 n = normalize(vNormal);
    vec3 albedo = mix(uBark, uCrown * (0.75 + 0.5 * vnoise(vWorld.xz * 3.0 + vWorld.y * 2.0)), vPart);
    vec3 color = shade(albedo, n, vWorld, 0.6, vPart * 0.6);
    vec3 view = safeNormalize(vWorld - cameraPosition);
    float edge = 1.0 - abs(dot(n, view));
    color += uMoonColor * albedo * edge * edge * max(dot(view, uMoonDir), 0.0) * 2.2;
    gl_FragColor = vec4(atmosphere(color, vWorld), 1.0);
  }
`;

// The crescent's fur and the tufts at the tree's foot: blades rooted on a surface in the mark's own
// space, combed along it, rippled by the wind and parted by the pointer.
const FUR_VERTEX = /* glsl */ `
  attribute vec3 aRoot;
  attribute vec3 aUp;
  attribute vec3 aComb;
  attribute vec4 aBlade;
  uniform vec3 uWindLocal;
  uniform vec4 uPushLocal;
  varying float vT;
  varying float vShade;
  varying vec3 vWorld;
  varying vec3 vNormal;
  ${NOISE}
  ${WIND_GLSL}
  void main() {
    float t = position.y;
    float height = aBlade.x;
    float width = aBlade.y;
    float seed = aBlade.z;
    float ripple = vnoise(aRoot.xy * 3.0 - vec2(uTime * 0.6, uTime * 0.2));
    vec3 bend = aComb * aBlade.w + uWindLocal * (0.05 + ripple * 0.18);
    bend += vec3(sin(uTime * 0.9 + seed * 50.0), cos(uTime * 0.7 + seed * 31.0), 0.0) * 0.012;
    vec3 away = aRoot - uPushLocal.xyz;
    float distance = length(away);
    float push = 1.0 - smoothstep(0.0, 0.32, distance);
    if (distance > 1e-4) bend += away / distance * push * uPushLocal.w * 1.2;
    vec3 local = aRoot + (aUp * t + bend * t * t * 0.5) * height;
    vec3 side = safeNormalize(cross(aUp, aComb));
    local += side * position.x * width;
    vec3 tangent = safeNormalize(aUp + bend * t);
    vec3 normal = safeNormalize(cross(side, tangent));
    vec3 surface = safeNormalize(aUp - aComb * dot(aUp, aComb));
    normal = safeNormalize(mix(normal, surface, 0.85));
    vec4 world = modelMatrix * vec4(local, 1.0);
    vWorld = world.xyz;
    vNormal = safeNormalize(mat3(modelMatrix) * normal);
    vT = t;
    vShade = fract(seed * 13.7);
    gl_Position = projectionMatrix * viewMatrix * world;
  }
`;

// The canopy: leaf sprigs on cards facing out from the crown's lobes, lit as a rounded crown and
// glowing where the moon shines through.
const CARD_VERTEX = /* glsl */ `
  attribute vec4 aCard;
  attribute vec4 aFacing;
  uniform vec3 uWindLocal;
  uniform float uTime;
  varying vec2 vUv;
  varying vec3 vWorld;
  varying vec3 vNormal;
  varying float vShade;
  ${NOISE}
  void main() {
    vec3 normal = safeNormalize(aFacing.xyz);
    vec3 helper = abs(normal.y) < 0.95 ? vec3(0.0, 1.0, 0.0) : vec3(1.0, 0.0, 0.0);
    vec3 right = safeNormalize(cross(helper, normal));
    vec3 up = cross(normal, right);
    float c = cos(aFacing.w);
    float s = sin(aFacing.w);
    vec3 r = right * c + up * s;
    vec3 u = up * c - right * s;
    vec3 centre = aCard.xyz;
    float lift = max(centre.y - 0.2, 0.0);
    float phase = uTime * 1.3 + aCard.x * 9.0 + aCard.y * 5.0;
    centre += uWindLocal * lift * (0.05 + 0.04 * sin(phase)) + vec3(sin(phase * 1.7), cos(phase * 1.3), 0.0) * 0.006;
    vec3 local = centre + (r * position.x + u * position.y) * aCard.w;
    vec4 world = modelMatrix * vec4(local, 1.0);
    vUv = uv;
    vWorld = world.xyz;
    vNormal = safeNormalize(mat3(modelMatrix) * normalize(normal + (r * position.x + u * position.y) * 0.6));
    vShade = fract(aFacing.w * 3.17);
    gl_Position = projectionMatrix * viewMatrix * world;
  }
`;

const CARD_FRAGMENT = /* glsl */ `
  uniform sampler2D tSprig;
  uniform vec3 uTint;
  uniform float uTime;
  varying vec2 vUv;
  varying vec3 vWorld;
  varying vec3 vNormal;
  varying float vShade;
  ${NOISE}
  ${LIGHT}
  void main() {
    vec4 sprig = texture2D(tSprig, vUv);
    if (sprig.a < 0.45) discard;
    vec3 n = normalize(vNormal);
    if (!gl_FrontFacing) n = -n;
    vec3 albedo = sprig.rgb * uTint * (0.75 + 0.5 * vShade);
    gl_FragColor = vec4(shade(albedo, n, vWorld, 0.8, 0.9), 1.0);
  }
`;

// Fireflies over the field, specks in the hollow and the pointer's swarm, as soft points of light.
const FLY_VERTEX = /* glsl */ `
  attribute vec4 aSeed;
  uniform float uPixel;
  uniform float uHalfTan;
  uniform vec3 uSwarm;
  uniform float uSwarmPresence;
  uniform float uLocalScale;
  varying float vGlow;
  varying float vHue;
  ${NOISE}
  ${GROUND}
  uniform float uTime;
  void main() {
    vec3 position3;
    float size;
    #if defined(SPECKS)
      vec2 inner = vec2(${INNER.x.toFixed(4)}, ${INNER.y.toFixed(4)});
      float fall = fract(aSeed.z + uTime * (0.02 + aSeed.w * 0.02));
      vec2 p = vec2(aSeed.x, aSeed.y) + vec2(fall * 0.22 + sin(uTime * 0.7 + aSeed.w * 20.0) * 0.02, -fall * 0.5);
      position3 = vec3(p, 0.05 + (aSeed.w - 0.5) * 0.3);
      float inside = 1.0 - smoothstep(${(CRESCENT.inner - 0.1).toFixed(3)}, ${(CRESCENT.inner - 0.02).toFixed(3)}, length(p - inner));
      vGlow = inside * sin(fall * 3.14159) * (0.35 + 0.35 * step(0.8, aSeed.w));
      size = (0.018 + aSeed.w * 0.014) * uLocalScale;
      vec4 view = modelViewMatrix * vec4(position3, 1.0);
    #elif defined(SWARM)
      float t = uTime * (0.8 + aSeed.z) + aSeed.w * 30.0;
      position3 = uSwarm + vec3(sin(t), sin(t * 1.3 + aSeed.x * 6.0) * 0.6, cos(t * 0.8 + aSeed.y * 6.0)) * (0.25 + aSeed.x * 0.35);
      vGlow = uSwarmPresence * (0.5 + 0.5 * sin(uTime * 3.0 + aSeed.w * 20.0));
      size = 0.05;
      vec4 view = viewMatrix * vec4(position3, 1.0);
    #else
      float ahead = mix(7.0, 40.0, aSeed.y * aSeed.y);
      vec2 root = vec2((aSeed.x * 2.0 - 1.0) * (ahead * uHalfTan + 1.0), uCameraZ - ahead);
      float t = uTime * (0.12 + aSeed.z * 0.18) + aSeed.w * 40.0;
      root += vec2(sin(t * 1.3 + aSeed.w * 9.0), cos(t * 0.9 + aSeed.z * 7.0)) * 0.9;
      position3 = vec3(root.x, groundHeight(root) + 0.25 + aSeed.z * 1.2 + sin(t * 2.1) * 0.18, root.y);
      vGlow = smoothstep(0.35, 0.95, sin(uTime * (0.7 + aSeed.x * 0.8) + aSeed.w * 31.0) * 0.5 + 0.5);
      size = 0.07;
      vec4 view = viewMatrix * vec4(position3, 1.0);
    #endif
    vHue = aSeed.w;
    gl_Position = projectionMatrix * view;
    gl_PointSize = clamp(uPixel * size / max(-view.z, 0.3), 1.0, 48.0);
  }
`;

const FLY_FRAGMENT = /* glsl */ `
  uniform vec3 uColor;
  uniform vec3 uColor2;
  uniform float uIntensity;
  varying float vGlow;
  varying float vHue;
  void main() {
    vec2 d = gl_PointCoord - 0.5;
    float falloff = exp(-dot(d, d) * 22.0);
    vec3 color = mix(uColor, uColor2, vHue);
    gl_FragColor = vec4(color * falloff * vGlow * uIntensity, 1.0);
  }
`;

const FULLSCREEN_VERTEX = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

// The night: a sky from the horizon's glow to a dark zenith, two layers of stars, a dusting of the
// galaxy and moonlight behind the mark, with the aurora laid over it from its own target.
const SKY_FRAGMENT = /* glsl */ `
  uniform vec3 uRight;
  uniform vec3 uUp;
  uniform vec3 uForward;
  uniform vec2 uTan;
  uniform vec3 uZenith;
  uniform vec3 uHorizon;
  uniform vec3 uAurora;
  uniform vec3 uAuroraHigh;
  uniform sampler2D tAurora;
  uniform vec3 uMoonGlow;
  uniform vec2 uGlowCenter;
  uniform float uGlowRadius;
  uniform float uAspect;
  uniform float uTime;
  varying vec2 vUv;
  ${NOISE}
  float stars(vec2 p, float density, float size) {
    vec2 cell = floor(p);
    vec2 local = fract(p) - 0.5;
    float r = hash12(cell);
    vec2 offset = vec2(hash12(cell + 7.1), hash12(cell + 3.7)) - 0.5;
    float d = length(local - offset * 0.7);
    float present = step(1.0 - density, r);
    float twinkle = 0.6 + 0.4 * sin(uTime * (0.8 + r * 3.0) + r * 60.0);
    return present * (1.0 - smoothstep(0.0, size, d)) * twinkle * (0.25 + 0.75 * hash12(cell + 1.3));
  }
  void main() {
    vec2 ndc = vUv * 2.0 - 1.0;
    vec3 ray = safeNormalize(uForward + ndc.x * uTan.x * uRight + ndc.y * uTan.y * uUp);
    float h = ray.y;
    float above = max(h, 0.0);
    vec3 color = mix(uHorizon, uZenith, sqrt(min(above * 3.0, 1.0)));
    color += uHorizon * 0.55 * exp(-abs(h) * 22.0);
    if (h < -0.03) {
      gl_FragColor = vec4(color, 1.0);
      return;
    }
    float azimuth = atan(ray.x, -ray.z + 1e-5);
    float elevation = asin(clamp(h, -1.0, 1.0));
    vec2 sphere = vec2(azimuth, elevation);
    float extinction = smoothstep(0.0, 0.12, h);
    float starLight = stars(sphere * 95.0, 0.22, 0.09) * 0.8 + stars(sphere * 34.0 + 11.0, 0.07, 0.12) * 1.8;
    vec3 band = vec3(0.0);
    float across = dot(ray, normalize(vec3(0.55, 0.62, 0.55))) * 5.0;
    float galaxy = exp(-across * across);
    band += uAuroraHigh * 0.05 * galaxy * vnoise(sphere * 9.0);
    color += (vec3(0.85, 0.92, 1.0) * starLight * (0.5 + galaxy * 0.8) + band) * extinction;
    color += texture2D(tAurora, vUv).rgb;
    color += uAurora * 0.05 * exp(-h * 9.0);
    vec2 d = (vUv - uGlowCenter) * vec2(uAspect, 1.0);
    float r2 = dot(d, d) / max(uGlowRadius * uGlowRadius, 1e-4);
    color += uMoonGlow * (exp(-r2 * 1.4) * 0.22 + exp(-r2 * 0.25) * 0.08);
    gl_FragColor = vec4(color, 1.0);
  }
`;

// The aurora: folded sheets of light marched through a band of the sky, green low down, rising
// through teal into violet, slowly turning over. The folds are a triangle-wave noise warped into
// itself; each step up the band carries some of the colour below it, which draws the rays. It is
// soft, so it is drawn at half resolution into its own target and laid over the sky.
const AURORA_FRAGMENT = /* glsl */ `
  uniform vec3 uRight;
  uniform vec3 uUp;
  uniform vec3 uForward;
  uniform vec2 uTan;
  uniform vec3 uAurora;
  uniform vec3 uAuroraMid;
  uniform vec3 uAuroraHigh;
  uniform float uTime;
  uniform float uStrength;
  varying vec2 vUv;
  float hash(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
  }
  float tri(float x) {
    return clamp(abs(fract(x) - 0.5), 0.01, 0.49);
  }
  vec2 tri2(vec2 p) {
    return vec2(tri(p.x) + tri(p.y), tri(p.y + tri(p.x)));
  }
  mat2 turn(float a) {
    float c = cos(a);
    float s = sin(a);
    return mat2(c, s, -s, c);
  }
  float sheets(vec2 p) {
    float amplitude = 1.8;
    float warp = 2.5;
    float sum = 0.0;
    p *= turn(p.x * 0.06);
    vec2 base = p;
    for (int i = 0; i < 5; i++) {
      vec2 fold = tri2(base * 1.85) * 0.75;
      fold *= turn(uTime * 0.11);
      p -= fold / warp;
      base *= 1.3;
      warp *= 0.45;
      amplitude *= 0.42;
      p *= 1.21 + (sum - 1.0) * 0.02;
      sum += tri(p.x + tri(p.y)) * amplitude;
      p *= -mat2(0.95534, 0.29552, -0.29552, 0.95534);
    }
    return clamp(1.0 / pow(max(sum * 29.0, 1e-3), 1.3), 0.0, 0.55);
  }
  void main() {
    vec2 ndc = vUv * 2.0 - 1.0;
    vec3 ray = normalize(uForward + ndc.x * uTan.x * uRight + ndc.y * uTan.y * uUp);
    if (ray.y < -0.05) {
      gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0);
      return;
    }
    vec3 sum = vec3(0.0);
    vec3 carried = vec3(0.0);
    float jitter = hash(gl_FragCoord.xy) * 0.006;
    for (int i = 0; i < STEPS; i++) {
      float fi = float(i) * 40.0 / float(STEPS);
      float altitude = 0.8 + pow(fi, 1.4) * 0.002;
      float travel = altitude / (ray.y * 1.15 + 0.3) - jitter * smoothstep(0.0, 15.0, fi);
      vec2 p = ray.zx * travel + vec2(uTime * 0.025, uTime * 0.008);
      float strength = sheets(p);
      strength = strength * strength * 3.3;
      vec3 tint = fi < 16.0 ? mix(uAurora, uAuroraMid, fi / 16.0) : mix(uAuroraMid, uAuroraHigh, min((fi - 16.0) / 20.0, 1.0));
      carried = mix(carried, tint * strength, 0.5);
      sum += carried * exp2(-fi * 0.065 - 2.5) * smoothstep(0.0, 5.0, fi) * 40.0 / float(STEPS);
    }
    sum *= clamp(ray.y * 15.0 + 0.4, 0.0, 1.0) * uStrength;
    gl_FragColor = vec4(sum, 1.0);
  }
`;

const SCRUB = /* glsl */ `
  vec3 scrub(vec3 c) {
    if (any(isnan(c)) || any(isinf(c)) || c.r != c.r || c.g != c.g || c.b != c.b) return vec3(0.0);
    return clamp(c, 0.0, 64.0);
  }
`;

const BRIGHT_FRAGMENT = /* glsl */ `
  uniform sampler2D tInput;
  uniform float uThreshold;
  varying vec2 vUv;
  ${SCRUB}
  void main() {
    vec3 c = scrub(texture2D(tInput, vUv).rgb);
    float luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
    gl_FragColor = vec4(c * smoothstep(uThreshold, uThreshold + 0.6, luma), 1.0);
  }
`;

const BLUR_FRAGMENT = /* glsl */ `
  uniform sampler2D tInput;
  uniform vec2 uDirection;
  varying vec2 vUv;
  void main() {
    vec3 sum = texture2D(tInput, vUv).rgb * 0.2270270270;
    sum += texture2D(tInput, vUv + uDirection * 1.3846153846).rgb * 0.3162162162;
    sum += texture2D(tInput, vUv - uDirection * 1.3846153846).rgb * 0.3162162162;
    sum += texture2D(tInput, vUv + uDirection * 3.2307692308).rgb * 0.0702702703;
    sum += texture2D(tInput, vUv - uDirection * 3.2307692308).rgb * 0.0702702703;
    gl_FragColor = vec4(sum, 1.0);
  }
`;

// Tone mapping, the bloom, and a soft scrim behind the hero's words so they read over the field.
const COMPOSITE_FRAGMENT = /* glsl */ `
  uniform sampler2D tScene;
  uniform sampler2D tBloomNear;
  uniform sampler2D tBloomFar;
  uniform float uTime;
  uniform float uExposure;
  uniform vec4 uScrim;
  uniform float uScrimStrength;
  uniform float uAspect;
  varying vec2 vUv;
  vec3 aces(vec3 x) {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
  }
  float dither(vec2 p) {
    return fract(sin(dot(p + fract(uTime), vec2(12.9898, 78.233))) * 43758.5453) - 0.5;
  }
  ${SCRUB}
  void main() {
    vec3 color = scrub(texture2D(tScene, vUv).rgb);
    color += scrub(texture2D(tBloomNear, vUv).rgb) * 0.65 + scrub(texture2D(tBloomFar, vUv).rgb) * 0.6;
    vec2 outside = max(uScrim.xy - vUv, vUv - uScrim.zw);
    outside = max(outside, 0.0) * vec2(uAspect, 1.0);
    float scrim = uScrimStrength * (1.0 - smoothstep(0.0, 0.16, length(outside)));
    color *= 1.0 - scrim;
    color = aces(color * uExposure);
    color = pow(color, vec3(1.0 / 2.2));
    color += dither(gl_FragCoord.xy) / 255.0;
    gl_FragColor = vec4(color, 1.0);
  }
`;

function fullscreenMaterial(fragmentShader, uniforms) {
  return new THREE.ShaderMaterial({ vertexShader: FULLSCREEN_VERTEX, fragmentShader, uniforms, depthTest: false, depthWrite: false });
}

// Geometry ---------------------------------------------------------------------------------------

// One blade: a tapering strip in `segments` steps to a point. x is across, y the height from 0 to 1.
function bladeGeometry(segments) {
  const positions = [];
  const indices = [];
  for (let i = 0; i < segments; i++) {
    const t = i / segments;
    const half = 0.5 * (1 - Math.pow(t, 1.4) * 0.8);
    positions.push(-half, t, 0, half, t, 0);
  }
  positions.push(0, 1, 0);
  for (let i = 0; i < segments - 1; i++) {
    const a = i * 2;
    indices.push(a, a + 1, a + 2, a + 1, a + 3, a + 2);
  }
  const last = (segments - 1) * 2;
  indices.push(last, last + 1, segments * 2);
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setIndex(indices);
  return geometry;
}

// The ground: rows spaced ever wider with distance, each as wide as the view is there.
function terrainGeometry() {
  const rows = 110;
  const columns = 130;
  const positions = [];
  const indices = [];
  for (let r = 0; r <= rows; r++) {
    const ahead = 1.5 * Math.pow(300 / 1.5, r / rows);
    const half = ahead * 1.7 + 14;
    for (let c = 0; c <= columns; c++) {
      const x = (c / columns * 2 - 1) * half;
      const z = CAMERA.z - ahead;
      positions.push(x, groundHeight(x, z), z);
    }
  }
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < columns; c++) {
      const a = r * (columns + 1) + c;
      const b = a + columns + 1;
      indices.push(a, b, a + 1, a + 1, b, b + 1);
    }
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setIndex(indices);
  geometry.computeVertexNormals();
  const normal = geometry.attributes.normal;
  if (normal.getY(0) < 0) for (let i = 0; i < normal.array.length; i++) normal.array[i] = -normal.array[i];
  return geometry;
}

// Where the ray from the crescent's centre at angle phi leaves the inner circle: the crescent's
// inner edge along it, as a distance from the centre. At the horns it reaches 1.
function innerEdge(phi) {
  const dx = Math.cos(phi);
  const dy = Math.sin(phi);
  const along = dx * INNER.x + dy * INNER.y;
  const disc = along * along - INNER.lengthSq() + CRESCENT.inner * CRESCENT.inner;
  return Math.min(1, along + Math.sqrt(Math.max(0, disc)));
}

// A point on the crescent's surface. phi runs around the crescent from horn to horn; around runs
// round its rounded cross-section, 0 at the outer rim and pi/2 on the front face.
function crescentPoint(phi, around, out) {
  const inner = innerEdge(phi);
  const width = Math.max(0, 1 - inner);
  const radius = (1 + inner) / 2 + width / 2 * Math.cos(around);
  const depth = CRESCENT.depth * Math.pow(width / CRESCENT.back, 0.55) + 0.004;
  return out.set(Math.cos(phi) * radius, Math.sin(phi) * radius, depth * Math.sin(around));
}

function crescentNormal(phi, around, out) {
  const e = 1e-3;
  const a = crescentPoint(phi + e, around, new THREE.Vector3()).sub(crescentPoint(phi - e, around, new THREE.Vector3()));
  const b = crescentPoint(phi, around + e, new THREE.Vector3()).sub(crescentPoint(phi, around - e, new THREE.Vector3()));
  out.crossVectors(b, a);
  const outward = new THREE.Vector3(Math.cos(phi) * Math.cos(around), Math.sin(phi) * Math.cos(around), Math.sin(around));
  if (out.lengthSq() < 1e-12) out.copy(outward);
  if (out.dot(outward) < 0) out.negate();
  return out.normalize();
}

function crescentGeometry(small) {
  const along = small ? 160 : 260;
  const around = 40;
  const positions = [];
  const normals = [];
  const uvs = [];
  const indices = [];
  const point = new THREE.Vector3();
  const normal = new THREE.Vector3();
  for (let i = 0; i <= along; i++) {
    const phi = PHI_START + (PHI_END - PHI_START) * i / along;
    for (let j = 0; j <= around; j++) {
      const angle = j / around * Math.PI * 2;
      crescentPoint(phi, angle, point);
      crescentNormal(phi, angle, normal);
      positions.push(point.x, point.y, point.z);
      normals.push(normal.x, normal.y, normal.z);
      uvs.push(i / along * 6, j / around);
    }
  }
  for (let i = 0; i < along; i++) {
    for (let j = 0; j < around; j++) {
      const a = i * (around + 1) + j;
      const b = a + around + 1;
      indices.push(a, b, a + 1, a + 1, b, b + 1);
    }
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
  geometry.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices);
  return geometry;
}

// The tree in the hollow, placed as on the icon: rooted on the inner rim low on the right, the
// trunk leaning up to a crown that reaches over the hollow towards the upper horn.
const TREE = {
  root: new THREE.Vector3(0.435, -0.52, 0.02),
  trunk: [[0.435, -0.54, 0.0], [0.41, -0.32, 0.03], [0.37, -0.08, 0.05], [0.4, 0.14, 0.04], [0.47, 0.31, 0.03]],
  limbs: [
    { points: [[0.38, 0.0, 0.05], [0.26, 0.07, 0.07], [0.15, 0.15, 0.06], [0.08, 0.2, 0.05]], radius: 0.016 },
    { points: [[0.42, 0.2, 0.04], [0.56, 0.33, 0.05], [0.7, 0.4, 0.04]], radius: 0.02 },
    { points: [[0.44, 0.26, 0.03], [0.38, 0.4, 0.02], [0.36, 0.5, 0.0]], radius: 0.016 },
  ],
  roots: [
    [[0.435, -0.5, 0.02], [0.35, -0.56, 0.05], [0.26, -0.6, 0.06]],
    [[0.44, -0.5, 0.02], [0.53, -0.57, 0.06], [0.63, -0.58, 0.05]],
    [[0.44, -0.5, 0.03], [0.46, -0.58, 0.13], [0.48, -0.62, 0.2]],
    [[0.43, -0.5, 0.0], [0.42, -0.58, -0.09], [0.4, -0.62, -0.15]],
  ],
  // The crown's lobes: centre x, y, z and radius.
  lobes: [[0.36, 0.44, 0.05, 0.2], [0.55, 0.52, 0.06, 0.23], [0.75, 0.46, 0.05, 0.21], [0.62, 0.34, 0.1, 0.19], [0.44, 0.34, 0.08, 0.17], [0.9, 0.36, 0.04, 0.15], [0.22, 0.37, 0.03, 0.12]],
  bush: [0.63, -0.46, 0.08, 0.09],
};

function tube(points, radius, taper, segments) {
  const curve = new THREE.CatmullRomCurve3(points.map((p) => new THREE.Vector3(...p)));
  const geometry = new THREE.TubeGeometry(curve, segments, radius, 8, false);
  const position = geometry.attributes.position;
  const count = position.count;
  const ring = 9;
  const center = new THREE.Vector3();
  const vertex = new THREE.Vector3();
  for (let i = 0; i < count; i++) {
    const along = Math.floor(i / ring) / segments;
    curve.getPointAt(Math.min(1, along), center);
    vertex.fromBufferAttribute(position, i).sub(center).multiplyScalar(1 - along * taper).add(center);
    position.setXYZ(i, vertex.x, vertex.y, vertex.z);
  }
  geometry.computeVertexNormals();
  return geometry;
}

function mergeGeometries(geometries) {
  const positions = [];
  const normals = [];
  const uvs = [];
  const indices = [];
  let offset = 0;
  for (const geometry of geometries) {
    const g = geometry.index ? geometry : geometry;
    positions.push(...g.attributes.position.array);
    normals.push(...g.attributes.normal.array);
    uvs.push(...(g.attributes.uv ? g.attributes.uv.array : new Float32Array(g.attributes.position.count * 2)));
    const index = g.index ? g.index.array : [...Array(g.attributes.position.count).keys()];
    for (const i of index) indices.push(i + offset);
    offset += g.attributes.position.count;
  }
  const merged = new THREE.BufferGeometry();
  merged.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  merged.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
  merged.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  merged.setIndex(indices);
  return merged;
}

function treeGeometry() {
  const parts = [tube(TREE.trunk, 0.05, 0.45, 40)];
  for (const limb of TREE.limbs) parts.push(tube(limb.points, limb.radius, 0.6, 16));
  for (const root of TREE.roots) parts.push(tube(root, 0.022, 0.75, 12));
  return mergeGeometries(parts);
}

// The leaves at the strings' ends. Each hangs from its stem at the origin along -y.
function leafShape(kind) {
  const shape = new THREE.Shape();
  const points = [];
  if (kind === 'maple') {
    const lobes = [0, 1.2, -1.2, 2.3, -2.3];
    const radius = 0.21;
    for (let i = 0; i <= 120; i++) {
      const theta = -Math.PI + i / 120 * Math.PI * 2;
      let reach = 0.32;
      for (const lobe of lobes) reach = Math.max(reach, Math.exp(-(((theta - lobe) / 0.3) ** 2)) * (lobe === 0 ? 1 : 0.86));
      reach *= 1 + 0.07 * Math.sin(theta * 26);
      const notch = Math.exp(-(((Math.abs(theta) - Math.PI) / 0.25) ** 2));
      const r = radius * reach * (1 - notch * 0.8);
      points.push(new THREE.Vector2(Math.sin(theta) * r, -radius * 0.95 - Math.cos(theta) * r));
    }
  } else {
    const length = kind === 'oak' ? 0.4 : 0.38;
    const width = kind === 'oak' ? 0.12 : 0.07;
    const edge = (t) => {
      const body = Math.pow(Math.sin(Math.PI * Math.min(1, t * 1.02)), kind === 'oak' ? 0.75 : 0.9);
      if (kind === 'oak') return width * body * (0.72 + 0.28 * Math.cos(t * Math.PI * 9));
      return width * body * (1 - 0.35 * t) * (1 + 0.04 * Math.sin(t * 60));
    };
    for (let i = 0; i <= 60; i++) {
      const t = i / 60;
      points.push(new THREE.Vector2(edge(t), -0.03 - t * length));
    }
    for (let i = 60; i >= 0; i--) {
      const t = i / 60;
      points.push(new THREE.Vector2(-edge(t), -0.03 - t * length));
    }
  }
  shape.setFromPoints(points);
  const geometry = new THREE.ShapeGeometry(shape, 12);
  geometry.computeBoundingBox();
  const box = geometry.boundingBox;
  const position = geometry.attributes.position;
  const uv = geometry.attributes.uv;
  for (let i = 0; i < position.count; i++) {
    const x = position.getX(i);
    const y = position.getY(i);
    const u = (x - box.min.x) / (box.max.x - box.min.x);
    const v = (y - box.min.y) / (box.max.y - box.min.y);
    uv.setXY(i, u, v);
    const across = x / Math.max(box.max.x, 1e-3);
    const down = -y / Math.max(-box.min.y, 1e-3);
    position.setZ(i, 0.05 * across * across + 0.07 * down * down);
  }
  // The stem.
  geometry.computeVertexNormals();
  return geometry;
}

// Background trees: three shapes of trunk and lumpy crown, low in polygons since they stand far off.
function hillTreeShapes() {
  const next = random(83);
  const shapes = [];
  for (let kind = 0; kind < 3; kind++) {
    const parts = [];
    const trunkHeight = 1.1 + kind * 0.35;
    const trunk = new THREE.CylinderGeometry(0.05, 0.1, trunkHeight, 6, 1, true);
    trunk.translate(0, trunkHeight / 2, 0);
    parts.push([trunk, 0]);
    const blobs = 6 + kind * 2;
    for (let i = 0; i < blobs; i++) {
      const radius = 0.38 + next() * 0.3 - kind * 0.04;
      const blob = new THREE.SphereGeometry(radius, 10, 7);
      const position = blob.attributes.position;
      const normal = blob.attributes.normal;
      const phase = [next() * 6, next() * 6, next() * 6];
      const direction = new THREE.Vector3();
      for (let v = 0; v < position.count; v++) {
        direction.fromBufferAttribute(normal, v);
        const lump = 1 + 0.12 * (Math.sin(direction.x * 5 + phase[0]) + Math.sin(direction.y * 4 + phase[1]) + Math.sin(direction.z * 6 + phase[2])) / 3;
        position.setXYZ(v, direction.x * radius * lump, direction.y * radius * lump * 0.85, direction.z * radius * lump);
      }
      const angle = next() * Math.PI * 2;
      const spread = kind === 2 ? 0.25 : 0.45;
      blob.translate(Math.cos(angle) * spread * next(), trunkHeight + 0.2 + next() * (0.7 + kind * 0.5), Math.sin(angle) * spread * next());
      parts.push([blob, 1]);
    }
    const merged = mergeGeometries(parts.map(([geometry]) => geometry));
    const part = [];
    for (const [geometry, value] of parts) for (let k = 0; k < geometry.attributes.position.count; k++) part.push(value);
    merged.setAttribute('aPart', new THREE.Float32BufferAttribute(part, 1));
    shapes.push(merged);
  }
  return shapes;
}

// The environment the beads and the crescent reflect: a night dome, the aurora's green and a moon.
function environment(renderer, colors) {
  const scene = new THREE.Scene();
  const dome = new THREE.Mesh(new THREE.SphereGeometry(10, 32, 16), new THREE.ShaderMaterial({
    side: THREE.BackSide,
    uniforms: { uZenith: { value: colors.zenith }, uHorizon: { value: colors.horizon }, uAurora: { value: colors.aurora }, uGround: { value: colors.groundTint } },
    vertexShader: /* glsl */ `
      varying vec3 vDirection;
      void main() {
        vDirection = normalize(position);
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */ `
      uniform vec3 uZenith;
      uniform vec3 uHorizon;
      uniform vec3 uAurora;
      uniform vec3 uGround;
      varying vec3 vDirection;
      void main() {
        float h = vDirection.y;
        vec3 color = h > 0.0 ? mix(uHorizon * 3.0, uZenith * 2.0, sqrt(h)) : mix(uHorizon, uGround, min(-h * 4.0, 1.0));
        float band = (h - 0.3) * 6.0;
        color += uAurora * exp(-band * band) * 0.8 * max(-vDirection.z, 0.0);
        gl_FragColor = vec4(color, 1.0);
      }
    `,
  }));
  scene.add(dome);
  const moon = new THREE.Mesh(new THREE.SphereGeometry(0.7, 16, 8), new THREE.MeshBasicMaterial({ color: new THREE.Color(6, 6.5, 6) }));
  moon.position.set(-3.5, 6, -7);
  scene.add(moon);
  const pmrem = new THREE.PMREMGenerator(renderer);
  const target = pmrem.fromScene(scene, 0.03);
  pmrem.dispose();
  return target;
}

// The mark ---------------------------------------------------------------------------------------

function furInstances(small) {
  const next = random(101);
  const count = small ? 4200 : 9500;
  const tufts = small ? 380 : 800;
  const roots = [];
  const ups = [];
  const combs = [];
  const blades = [];
  const point = new THREE.Vector3();
  const normal = new THREE.Vector3();
  const tangent = new THREE.Vector3();
  const across = new THREE.Vector3();
  const a = new THREE.Vector3();
  const b = new THREE.Vector3();
  let placed = 0;
  while (placed < count) {
    const phi = PHI_START + (PHI_END - PHI_START) * next();
    const width = 1 - innerEdge(phi);
    if (next() > width / CRESCENT.back) continue;
    const around = -0.35 + next() * (Math.PI + 0.7);
    crescentPoint(phi, around, point);
    crescentNormal(phi, around, normal);
    crescentPoint(phi + 1e-3, around, a);
    crescentPoint(phi - 1e-3, around, b);
    tangent.subVectors(a, b).normalize();
    crescentPoint(phi, around + 1e-3, a);
    crescentPoint(phi, around - 1e-3, b);
    across.subVectors(a, b).normalize();
    const swirl = 0.35 + Math.sin(phi * 5 + around * 2) * 0.3 + (next() - 0.5) * 0.3;
    const comb = tangent.clone().multiplyScalar(Math.cos(swirl)).addScaledVector(across, Math.sin(swirl)).normalize();
    const thin = Math.min(1, width / 0.25);
    roots.push(point.x, point.y, point.z);
    ups.push(normal.x, normal.y, normal.z);
    combs.push(comb.x, comb.y, comb.z);
    blades.push((0.05 + next() * 0.055) * (0.5 + 0.5 * thin), 0.022 + next() * 0.012, next(), 0.9 + next() * 0.6);
    placed++;
  }
  // Tufts round the tree's foot and along the inner rim beside it, standing up into the hollow.
  for (let i = 0; i < tufts; i++) {
    const angle = -Math.PI / 2 + (next() - 0.5) * 1.3;
    const r = CRESCENT.inner - 0.02;
    const x = INNER.x + Math.cos(angle) * r;
    const y = INNER.y + Math.sin(angle) * r;
    const up = new THREE.Vector3(INNER.x - x, INNER.y - y, 0).normalize().multiplyScalar(0.8).add(new THREE.Vector3(0, 0.5, 0.25)).normalize();
    const comb = new THREE.Vector3(next() - 0.5, 0.3, next() - 0.5).normalize();
    roots.push(x, y, (next() - 0.5) * 0.24);
    ups.push(up.x, up.y, up.z);
    combs.push(comb.x, comb.y, comb.z);
    blades.push(0.08 + next() * 0.12, 0.014 + next() * 0.01, next(), 0.35 + next() * 0.4);
  }
  return { roots, ups, combs, blades, count: count + tufts };
}

function cardInstances(small) {
  const next = random(131);
  const cards = [];
  const facings = [];
  const place = (lobe, count, size) => {
    const [cx, cy, cz, radius] = lobe;
    for (let i = 0; i < count; i++) {
      const direction = new THREE.Vector3(next() * 2 - 1, next() * 2 - 1, next() * 2 - 1);
      if (direction.lengthSq() < 1e-4) direction.set(0, 1, 0);
      direction.normalize();
      direction.y = direction.y * 0.8 + 0.12;
      const depth = Math.cbrt(next()) * 0.35 + 0.65;
      cards.push(cx + direction.x * radius * depth, cy + direction.y * radius * depth * 0.8, cz + direction.z * radius * depth * 0.9, size * (0.8 + next() * 0.5));
      facings.push(direction.x, direction.y, direction.z, next() * Math.PI * 2);
    }
  };
  const perLobe = small ? 110 : 220;
  for (const lobe of TREE.lobes) place(lobe, Math.round(perLobe * lobe[3] / 0.16), 0.12);
  place(TREE.bush, small ? 60 : 120, 0.07);
  return { cards, facings, count: cards.length / 4 };
}

// The strings: verlet chains in the mark's space, pinned under the crescent.
function makeStrings() {
  return STRINGS.map((string) => {
    const angle = THREE.MathUtils.degToRad(string.angle);
    const anchor = new THREE.Vector3(Math.cos(angle) * 0.975, Math.sin(angle) * 0.975, 0.02);
    const points = [];
    for (let i = 0; i < string.nodes; i++) {
      const p = anchor.clone().add(new THREE.Vector3(0, -i * LINK, 0));
      points.push({ p, prev: p.clone() });
    }
    return { ...string, anchor, points, twist: 0, spin: 0, phase: string.angle * 0.1 };
  });
}

function stepStrings(strings, dt, time, forces) {
  const { gravity, wind, gust, pointer, push, kick } = forces;
  const step = new THREE.Vector3();
  for (const string of strings) {
    const { points } = string;
    for (let i = 1; i < points.length; i++) {
      const node = points[i];
      step.subVectors(node.p, node.prev).multiplyScalar(0.985);
      node.prev.copy(node.p);
      const flutter = Math.sin(time * 2.3 + i * 0.8 + string.phase) * 0.6 + Math.sin(time * 5.1 + i * 1.7) * 0.25;
      const force = gravity.clone().multiplyScalar(3.2)
        .addScaledVector(wind, (0.7 + gust * 1.4) * (0.6 + 0.4 * flutter) * (i / points.length));
      if (push > 0) {
        const away = node.p.clone().sub(pointer);
        away.z *= 0.5;
        const d2 = away.lengthSq();
        if (d2 > 1e-8 && d2 < 0.16) force.addScaledVector(away.normalize(), push * 9 * (1 - Math.sqrt(d2) / 0.4));
      }
      if (kick) force.add(kick);
      node.p.add(step).addScaledVector(force, dt * dt);
    }
    for (let iteration = 0; iteration < 8; iteration++) {
      points[0].p.copy(string.anchor);
      points[0].prev.copy(string.anchor);
      for (let i = 1; i < points.length; i++) {
        const a = points[i - 1].p;
        const b = points[i].p;
        step.subVectors(b, a);
        const length = step.length();
        if (length < 1e-6) continue;
        const correction = (length - LINK) / length;
        if (i === 1) b.addScaledVector(step, -correction);
        else {
          a.addScaledVector(step, correction * 0.5);
          b.addScaledVector(step, -correction * 0.5);
        }
      }
    }
    // The leaf turns about its string: springing back, stirred by the wind, flung by a kick.
    const torque = -string.twist * 6 - string.spin * 1.6 + Math.sin(time * 1.9 + string.phase) * (1.2 + gust * 3) + (kick ? kick.x * 4 : 0);
    string.spin += torque * dt;
    string.twist += string.spin * dt;
  }
}

// Layout -----------------------------------------------------------------------------------------

function textRects(text) {
  const rects = [];
  const range = document.createRange();
  const walker = document.createTreeWalker(text, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => (node.nodeValue.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
  });
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    range.selectNodeContents(node);
    for (const rect of range.getClientRects()) rects.push(rect);
  }
  for (const element of text.querySelectorAll('a, button, input, select, img, svg, .dw-command, .dw-badge')) rects.push(element.getBoundingClientRect());
  return rects.filter((rect) => rect.width > 0 && rect.height > 0);
}

// The largest spot clear of the text: beside all of it, beside everything above the buttons, beside
// the title rows above the summary, or above it all where the stylesheet has left room on a phone. Also returns the text's box, for the
// scrim. Both are relative to the art.
function placement(root) {
  const hero = root.closest('.dw-hero') || root.parentElement;
  const box = root.getBoundingClientRect();
  const text = hero.querySelector('.dw-hero__text') || hero.querySelector('.dw-shell');
  const strip = hero.querySelector('.dw-strip');
  const shellElement = hero.querySelector('.dw-hero__grid') || hero.querySelector('.dw-shell') || hero;
  const shellStyle = getComputedStyle(shellElement);
  const shellBox = shellElement.getBoundingClientRect();
  const shell = strip ? strip.getBoundingClientRect() : { left: shellBox.left + parseFloat(shellStyle.paddingLeft), right: shellBox.right - parseFloat(shellStyle.paddingRight) };
  const summary = hero.querySelector('.dw-hero__summary');
  const floor = strip ? strip.getBoundingClientRect().top : box.bottom - 24;
  const rects = text ? textRects(text) : [];
  if (!rects.length) return { x: box.width * 0.75, y: box.height * 0.45, size: Math.min(box.width * 0.3, box.height * 0.8), above: false, text: null };
  const gap = 32;
  const right = Math.max(...rects.map((rect) => rect.right));
  const left = Math.min(...rects.map((rect) => rect.left));
  const top = Math.min(...rects.map((rect) => rect.top));
  const bottom = Math.max(...rects.map((rect) => rect.bottom));
  const summaryTop = summary ? summary.getBoundingClientRect().top : floor;
  const headRects = rects.filter((rect) => rect.bottom <= summaryTop + 1);
  const headRight = headRects.length ? Math.max(...headRects.map((rect) => rect.right)) : right;
  const button = text.querySelector('.dw-button');
  const buttonsTop = button ? button.getBoundingClientRect().top : floor;
  const upperRects = rects.filter((rect) => rect.bottom <= buttonsTop + 1);
  const upperRight = upperRects.length ? Math.max(...upperRects.map((rect) => rect.right)) : right;
  const candidates = [
    { x0: right + gap, x1: shell.right, y0: box.top + 12, y1: floor - 8, above: false },
    { x0: upperRight + gap, x1: shell.right, y0: box.top + 10, y1: buttonsTop - 10, above: false },
    { x0: headRight + gap, x1: shell.right, y0: box.top + 10, y1: summaryTop - 10, above: false },
    { x0: shell.left, x1: shell.right, y0: box.top + 8, y1: top - 10, above: true },
  ].map((region) => {
    const width = region.x1 - region.x0;
    const height = region.y1 - region.y0;
    return { ...region, size: Math.max(0, Math.min(height, width / MARK_ASPECT)) };
  });
  const best = candidates.reduce((a, b) => (b.size > a.size ? b : a));
  const size = Math.min(best.size, 520);
  const markWidth = size * MARK_ASPECT;
  const x = best.above ? (best.x0 + best.x1) / 2 : Math.min(best.x1 - markWidth / 2, (best.x0 + best.x1) / 2 + (best.x1 - best.x0 - markWidth) * 0.3);
  return {
    x: x - box.left,
    y: (best.y0 + best.y1) / 2 - box.top,
    size,
    above: best.above,
    text: { left: left - box.left, right: right - box.left, top: top - box.top, bottom: Math.max(bottom, floor) - box.top },
  };
}

// The mount --------------------------------------------------------------------------------------

function mount(root) {
  const still = document.createElement('img');
  still.className = 'gm-hero__still';
  still.alt = '';
  still.decoding = 'async';
  still.src = new URL('../img/greenmote-mark.webp', import.meta.url).href;
  root.append(still);

  function placeStill() {
    const spot = placement(root);
    const height = spot.size * (1 + STILL_MARGIN * 2);
    const width = spot.size * (MARK_ASPECT + STILL_MARGIN * 2);
    Object.assign(still.style, {
      left: `${spot.x - width / 2}px`,
      top: `${spot.y - height / 2}px`,
      width: `${width}px`,
      height: `${height}px`,
    });
    root.classList.add('is-placed');
  }

  const canvas = document.createElement('canvas');
  canvas.className = 'gm-hero__canvas';
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: false, alpha: false, powerPreference: 'high-performance' });
  } catch {
    placeStill();
    return;
  }
  if (!renderer.capabilities.isWebGL2) {
    renderer.dispose();
    placeStill();
    return;
  }
  renderer.autoClear = false;
  renderer.outputColorSpace = THREE.LinearSRGBColorSpace;
  root.append(canvas);

  const small = Math.min(innerWidth, innerHeight) < 700;
  const floatTargets = renderer.extensions.has('EXT_color_buffer_float') || renderer.extensions.has('EXT_color_buffer_half_float');
  const targetType = floatTargets ? THREE.HalfFloatType : THREE.UnsignedByteType;
  const makeTarget = () => new THREE.WebGLRenderTarget(1, 1, { type: targetType, depthBuffer: false });
  const sceneTarget = new THREE.WebGLRenderTarget(1, 1, { type: targetType, samples: 4 });
  const bloomTargets = [makeTarget(), makeTarget(), makeTarget(), makeTarget()];
  const auroraTarget = makeTarget();

  // Colours, from the palette where it has them.
  const accent = cssColor('--dw-accent', '#8bd67f');
  const bg0 = cssColor('--dw-bg-0', '#060d07');
  const colors = {
    zenith: bg0.clone().lerp(new THREE.Color('#010409'), 0.55).multiplyScalar(0.8),
    horizon: new THREE.Color('#0c2a2c').lerp(accent, 0.08).multiplyScalar(0.55),
    aurora: accent.clone().lerp(new THREE.Color('#2cf07a'), 0.6),
    auroraMid: new THREE.Color('#14b8a8'),
    auroraHigh: new THREE.Color('#7a3ce0'),
    groundTint: new THREE.Color('#050d06'),
    sky: new THREE.Color('#1a3a48').multiplyScalar(0.55),
    key: new THREE.Color('#aac4ff').multiplyScalar(0.55),
    moon: new THREE.Color('#d8ecff').multiplyScalar(0.9),
    base: new THREE.Color('#0b2410'),
    tip: accent.clone().lerp(new THREE.Color('#b8e070'), 0.35),
    lamp: new THREE.Color('#e8ff9a'),
    glow: new THREE.Color('#d4ff8a'),
    fly: new THREE.Color('#e4ff7a'),
    fly2: accent.clone().lerp(new THREE.Color('#9dffcf'), 0.5),
    moonGlow: new THREE.Color('#c9e8d0'),
  };
  const fog = colors.horizon.clone().multiplyScalar(0.72);
  const mist = new THREE.Color('#35505a').multiplyScalar(0.42);

  const camera = new THREE.PerspectiveCamera(CAMERA.fov, 1, 0.1, 400);
  const cameraTarget = new THREE.Vector3();
  const setCamera = (x, y) => {
    camera.position.set(x, CAMERA.y + y, CAMERA.z);
    cameraTarget.set(x * 0.4, CAMERA.y + y + Math.tan(CAMERA.pitch) * 50, CAMERA.z - 50);
    camera.lookAt(cameraTarget);
    camera.updateMatrixWorld();
  };
  setCamera(0, 0);

  const scene = new THREE.Scene();
  const envTarget = environment(renderer, colors);
  scene.environment = envTarget.texture;

  // Uniforms every custom shader shares: time, wind, light and air.
  const shared = {
    uTime: { value: 0 },
    uCameraZ: { value: CAMERA.z },
    uWind: { value: WIND.clone() },
    uGust: { value: new THREE.Vector4(0, 0, 99, 0) },
    uSky: { value: colors.sky },
    uGroundTint: { value: colors.groundTint },
    uKeyDir: { value: new THREE.Vector3(0.35, 0.75, 0.55).normalize() },
    uKeyColor: { value: colors.key },
    uMoonDir: { value: new THREE.Vector3(-0.3, 0.55, -1).normalize() },
    uMoonColor: { value: colors.moon },
    uLampPos: { value: new THREE.Vector3(0, -50, 0) },
    uLampColor: { value: colors.lamp },
    uLamp: { value: 0 },
    uGlowPos: { value: new THREE.Vector3(0, -50, 0) },
    uGlowColor: { value: colors.glow },
    uGlow: { value: 0 },
    uFog: { value: fog },
    uMist: { value: mist },
    uFogDensity: { value: 0.0125 },
    uHalfTan: { value: 1 },
  };

  // The sky.
  const quad = new THREE.PlaneGeometry(2, 2);
  const skyUniforms = {
    uTime: shared.uTime,
    uRight: { value: new THREE.Vector3() },
    uUp: { value: new THREE.Vector3() },
    uForward: { value: new THREE.Vector3() },
    uTan: { value: new THREE.Vector2(1, 1) },
    uZenith: { value: colors.zenith },
    uHorizon: { value: colors.horizon },
    uAurora: { value: colors.aurora },
    uAuroraHigh: { value: colors.auroraHigh },
    tAurora: { value: auroraTarget.texture },
    uMoonGlow: { value: colors.moonGlow },
    uGlowCenter: { value: new THREE.Vector2(0.75, 0.5) },
    uGlowRadius: { value: 0.3 },
    uAspect: { value: 1 },
  };
  const sky = new THREE.Mesh(quad, fullscreenMaterial(SKY_FRAGMENT, skyUniforms));
  sky.frustumCulled = false;
  sky.renderOrder = -10;
  scene.add(sky);

  // The ground and the field.
  const grassColors = { uBase: { value: colors.base }, uTip: { value: colors.tip } };
  const terrain = new THREE.Mesh(terrainGeometry(), new THREE.ShaderMaterial({
    vertexShader: TERRAIN_VERTEX,
    fragmentShader: TERRAIN_FRAGMENT,
    uniforms: { ...shared, ...grassColors },
    side: THREE.DoubleSide,
  }));
  terrain.frustumCulled = false;
  scene.add(terrain);

  const bladeBudget = small ? 22000 : 56000;
  const field = bladeGeometry(4);
  const spots = new Float32Array(bladeBudget * 4);
  const spotRandom = random(7);
  for (let i = 0; i < bladeBudget; i++) {
    spots[i * 4] = spotRandom() * 2 - 1;
    spots[i * 4 + 1] = spotRandom();
    spots[i * 4 + 2] = spotRandom();
    spots[i * 4 + 3] = spotRandom();
  }
  field.setAttribute('aSpot', new THREE.InstancedBufferAttribute(spots, 4));
  const fieldRoots = new THREE.InstancedBufferAttribute(new Float32Array(bladeBudget * 4), 4);
  field.setAttribute('aRoot', fieldRoots);
  let fieldHalfTan = -1;
  // Stand each blade on the ground, across the view as wide as it is now.
  function placeField(halfTan) {
    if (Math.abs(halfTan - fieldHalfTan) < 1e-3) return;
    fieldHalfTan = halfTan;
    const roots = fieldRoots.array;
    for (let i = 0; i < bladeBudget; i++) {
      const ahead = FIELD.near * FIELD.far / (FIELD.far - spots[i * 4 + 1] * (FIELD.far - FIELD.near));
      const x = spots[i * 4] * (ahead * halfTan + 2.5);
      const z = CAMERA.z - ahead;
      roots[i * 4] = x;
      roots[i * 4 + 1] = z;
      roots[i * 4 + 2] = groundHeight(x, z);
      roots[i * 4 + 3] = vnoise(x * 0.6, z * 0.6);
    }
    fieldRoots.needsUpdate = true;
  }
  field.instanceCount = bladeBudget;
  const fieldUniforms = {
    ...shared,
    ...grassColors,
    uNear: { value: FIELD.near },
    uFar: { value: FIELD.far },
    uPush: { value: new THREE.Vector4(0, -100, 0, 1.8) },
  };
  const fieldMesh = new THREE.Mesh(field, new THREE.ShaderMaterial({
    vertexShader: FIELD_VERTEX,
    fragmentShader: GRASS_FRAGMENT,
    uniforms: fieldUniforms,
    defines: { FIELD: '' },
    side: THREE.DoubleSide,
  }));
  fieldMesh.frustumCulled = false;
  scene.add(fieldMesh);

  // The trees on the hills, in three bands of distance.
  const treeRandom = random(19);
  const shapes = hillTreeShapes();
  const bands = [[26, 40, 5], [42, 70, 14], [76, 140, 26]];
  for (const shape of shapes) {
    const instances = [];
    const grounds = [];
    for (const [near, far, count] of bands) {
      for (let i = 0; i < count; i++) {
        const ahead = near + treeRandom() * (far - near);
        const x = (treeRandom() * 2 - 1) * (ahead * 1.45 + 6);
        instances.push(x, CAMERA.z - ahead, (0.55 + treeRandom() * 0.65) * (1 + ahead * 0.012), treeRandom() * Math.PI * 2);
        grounds.push(groundHeight(x, CAMERA.z - ahead));
      }
    }
    const geometry = new THREE.InstancedBufferGeometry();
    geometry.index = shape.index;
    geometry.setAttribute('position', shape.attributes.position);
    geometry.setAttribute('normal', shape.attributes.normal);
    geometry.setAttribute('aPart', shape.attributes.aPart);
    geometry.setAttribute('aTree', new THREE.InstancedBufferAttribute(new Float32Array(instances), 4));
    geometry.setAttribute('aGround', new THREE.InstancedBufferAttribute(new Float32Array(grounds), 1));
    geometry.instanceCount = instances.length / 4;
    const trees = new THREE.Mesh(geometry, new THREE.ShaderMaterial({
      vertexShader: TREE_VERTEX,
      fragmentShader: TREE_FRAGMENT,
      uniforms: { ...shared, uBark: { value: new THREE.Color('#140d08') }, uCrown: { value: new THREE.Color('#0c2211') } },
    }));
    trees.frustumCulled = false;
    scene.add(trees);
  }

  // The mark: the crescent with its fur, the tree, the strings.
  const mark = new THREE.Group();
  scene.add(mark);
  const strands = strandMaps(small);
  // Matte, like the fur over it: no specular highlight to glint.
  const crescent = new THREE.Mesh(crescentGeometry(small), new THREE.MeshLambertMaterial({
    color: new THREE.Color('#2f6a2c'),
    map: strands.map,
    normalMap: strands.normal,
    normalScale: new THREE.Vector2(0.6, 0.6),
  }));
  mark.add(crescent);

  const fur = furInstances(small);
  const furGeometry = bladeGeometry(3);
  furGeometry.setAttribute('aRoot', new THREE.InstancedBufferAttribute(new Float32Array(fur.roots), 3));
  furGeometry.setAttribute('aUp', new THREE.InstancedBufferAttribute(new Float32Array(fur.ups), 3));
  furGeometry.setAttribute('aComb', new THREE.InstancedBufferAttribute(new Float32Array(fur.combs), 3));
  furGeometry.setAttribute('aBlade', new THREE.InstancedBufferAttribute(new Float32Array(fur.blades), 4));
  furGeometry.instanceCount = fur.count;
  const markLocal = {
    uWindLocal: { value: new THREE.Vector3() },
    uPushLocal: { value: new THREE.Vector4(0, 0, 9, 0) },
  };
  const furMesh = new THREE.Mesh(furGeometry, new THREE.ShaderMaterial({
    vertexShader: FUR_VERTEX,
    fragmentShader: GRASS_FRAGMENT,
    uniforms: { ...shared, ...markLocal, uBase: { value: new THREE.Color('#0a2610') }, uTip: { value: accent.clone().lerp(new THREE.Color('#5fbf45'), 0.5) } },
    side: THREE.DoubleSide,
  }));
  furMesh.frustumCulled = false;
  mark.add(furMesh);

  const bark = barkMaps();
  const tree = new THREE.Mesh(treeGeometry(), new THREE.MeshStandardMaterial({
    color: new THREE.Color('#b4553a'),
    map: bark.map,
    normalMap: bark.normal,
    roughness: 0.88,
    envMapIntensity: 0.3,
  }));
  mark.add(tree);

  const leafCards = cardInstances(small);
  const cardGeometry = new THREE.InstancedBufferGeometry();
  const plane = new THREE.PlaneGeometry(1, 1);
  cardGeometry.index = plane.index;
  cardGeometry.setAttribute('position', plane.attributes.position);
  cardGeometry.setAttribute('uv', plane.attributes.uv);
  cardGeometry.setAttribute('aCard', new THREE.InstancedBufferAttribute(new Float32Array(leafCards.cards), 4));
  cardGeometry.setAttribute('aFacing', new THREE.InstancedBufferAttribute(new Float32Array(leafCards.facings), 4));
  cardGeometry.instanceCount = leafCards.count;
  const canopy = new THREE.Mesh(cardGeometry, new THREE.ShaderMaterial({
    vertexShader: CARD_VERTEX,
    fragmentShader: CARD_FRAGMENT,
    uniforms: { ...shared, ...markLocal, tSprig: { value: sprigTexture() }, uTint: { value: new THREE.Color(1.25, 1.35, 1.15) } },
    side: THREE.DoubleSide,
  }));
  canopy.frustumCulled = false;
  mark.add(canopy);

  const strings = makeStrings();
  const beadCount = strings.reduce((sum, string) => sum + string.points.length - 1, 0);
  const beads = new THREE.InstancedMesh(new THREE.IcosahedronGeometry(1, 3), new THREE.MeshPhysicalMaterial({
    color: new THREE.Color('#2f7a33'),
    map: mossTexture(),
    roughness: 0.42,
    clearcoat: 0.7,
    clearcoatRoughness: 0.3,
    sheen: 0.6,
    sheenColor: accent,
    envMapIntensity: 0.9,
  }), beadCount);
  beads.frustumCulled = false;
  mark.add(beads);
  const cords = new THREE.InstancedMesh(new THREE.CylinderGeometry(1, 1, 1, 6), new THREE.MeshStandardMaterial({ color: new THREE.Color('#0d1a0c'), roughness: 0.8 }), beadCount);
  cords.frustumCulled = false;
  mark.add(cords);
  const leafMaterial = new THREE.MeshStandardMaterial({
    map: veinTexture(),
    side: THREE.DoubleSide,
    roughness: 0.62,
    envMapIntensity: 0.5,
    emissive: new THREE.Color('#0e2a0c'),
  });
  for (const string of strings) {
    string.leafMesh = new THREE.Mesh(leafShape(string.leaf), leafMaterial);
    mark.add(string.leafMesh);
  }

  // The specks in the hollow.
  const speckCount = small ? 40 : 70;
  const speckSeeds = new Float32Array(speckCount * 4);
  const speckRandom = random(149);
  for (let i = 0; i < speckCount; i++) {
    const angle = speckRandom() * Math.PI * 2;
    const r = Math.sqrt(speckRandom()) * (CRESCENT.inner - 0.08);
    speckSeeds[i * 4] = INNER.x + Math.cos(angle) * r;
    speckSeeds[i * 4 + 1] = INNER.y + Math.sin(angle) * r + 0.25;
    speckSeeds[i * 4 + 2] = speckRandom();
    speckSeeds[i * 4 + 3] = speckRandom();
  }
  const flyUniforms = (color, color2, intensity) => ({
    ...shared,
    uPixel: { value: 400 },
    uSwarm: { value: new THREE.Vector3() },
    uSwarmPresence: { value: 0 },
    uLocalScale: { value: 1 },
    uColor: { value: color },
    uColor2: { value: color2 },
    uIntensity: { value: intensity },
  });
  const points = (seeds, uniforms, defines) => {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(seeds.length / 4 * 3), 3));
    geometry.setAttribute('aSeed', new THREE.BufferAttribute(seeds, 4));
    const object = new THREE.Points(geometry, new THREE.ShaderMaterial({
      vertexShader: FLY_VERTEX,
      fragmentShader: FLY_FRAGMENT,
      uniforms,
      defines,
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
    }));
    object.frustumCulled = false;
    object.renderOrder = 5;
    return object;
  };
  const speckUniforms = flyUniforms(new THREE.Color('#dfffb0'), colors.fly, 1.6);
  const specks = points(speckSeeds, speckUniforms, { SPECKS: '' });
  mark.add(specks);

  const flyCount = small ? 70 : 150;
  const flySeeds = new Float32Array(flyCount * 4);
  const flyRandom = random(157);
  for (let i = 0; i < flySeeds.length; i++) flySeeds[i] = flyRandom();
  const fieldFlyUniforms = flyUniforms(colors.fly, colors.fly2, 3.2);
  scene.add(points(flySeeds, fieldFlyUniforms, {}));

  const swarmSeeds = new Float32Array(18 * 4);
  const swarmRandom = random(163);
  for (let i = 0; i < swarmSeeds.length; i++) swarmSeeds[i] = swarmRandom();
  const swarmUniforms = flyUniforms(colors.fly, new THREE.Color('#fff4b0'), 3.6);
  scene.add(points(swarmSeeds, swarmUniforms, { SWARM: '' }));

  // Lights for the standard materials: the same key, moon, sky, lamp and glow the shaders use.
  const key = new THREE.DirectionalLight(colors.key.clone().multiplyScalar(4.2), 1);
  const rim = new THREE.DirectionalLight(colors.moon.clone().lerp(colors.aurora, 0.25).multiplyScalar(2.2), 1);
  const hemi = new THREE.HemisphereLight(colors.sky.clone().multiplyScalar(4), colors.groundTint.clone().multiplyScalar(4), 1);
  const lamp = new THREE.PointLight(colors.lamp, 0, 0, 2);
  const glow = new THREE.PointLight(colors.glow, 0, 0, 2);
  scene.add(key, key.target, rim, rim.target, hemi, lamp, glow);

  // Post-processing.
  const postScene = new THREE.Scene();
  const postCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  const postQuad = new THREE.Mesh(quad);
  postQuad.frustumCulled = false;
  postScene.add(postQuad);
  const brightMaterial = fullscreenMaterial(BRIGHT_FRAGMENT, { tInput: { value: sceneTarget.texture }, uThreshold: { value: 0.85 } });
  const blurMaterial = fullscreenMaterial(BLUR_FRAGMENT, { tInput: { value: null }, uDirection: { value: new THREE.Vector2() } });
  const copyMaterial = fullscreenMaterial(/* glsl */ `
    uniform sampler2D tInput;
    varying vec2 vUv;
    void main() { gl_FragColor = texture2D(tInput, vUv); }
  `, { tInput: { value: null } });
  const auroraMaterial = fullscreenMaterial(AURORA_FRAGMENT, {
    uRight: skyUniforms.uRight,
    uUp: skyUniforms.uUp,
    uForward: skyUniforms.uForward,
    uTan: skyUniforms.uTan,
    uAurora: { value: colors.aurora },
    uAuroraMid: { value: colors.auroraMid },
    uAuroraHigh: { value: colors.auroraHigh },
    uTime: shared.uTime,
    uStrength: { value: 0.62 },
  });
  auroraMaterial.defines = { STEPS: small ? 24 : 36 };
  const compositeMaterial = fullscreenMaterial(COMPOSITE_FRAGMENT, {
    tScene: { value: sceneTarget.texture },
    tBloomNear: { value: bloomTargets[0].texture },
    tBloomFar: { value: bloomTargets[2].texture },
    uTime: shared.uTime,
    uExposure: { value: 1.55 },
    uScrim: { value: new THREE.Vector4(0, 0, 0, 0) },
    uScrimStrength: { value: 0 },
    uAspect: { value: 1 },
  });
  function pass(material, target) {
    postQuad.material = material;
    renderer.setRenderTarget(target);
    renderer.render(postScene, postCamera);
  }
  function blur(target, scratch, radius) {
    blurMaterial.uniforms.tInput.value = target.texture;
    blurMaterial.uniforms.uDirection.value.set(radius / target.width, 0);
    pass(blurMaterial, scratch);
    blurMaterial.uniforms.tInput.value = scratch.texture;
    blurMaterial.uniforms.uDirection.value.set(0, radius / target.height);
    pass(blurMaterial, target);
  }

  // Layout: the size of everything, and where the mark stands.
  const quality = { level: 1, slow: 0, blades: 1 };
  let width = 1;
  let height = 1;
  let scale = 1;
  let place = { x: 0, y: 0, size: 0, above: false, text: null };
  const anchor = new THREE.Vector3();
  const raycaster = new THREE.Raycaster();
  const markPlane = new THREE.Plane(new THREE.Vector3(0, 0, 1), -MARK_Z);
  const groundPlane = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
  const ndc = new THREE.Vector2();
  const tmp = new THREE.Vector3();

  function layout() {
    const rect = root.getBoundingClientRect();
    width = Math.max(1, Math.round(rect.width));
    height = Math.max(1, Math.round(rect.height));
    const dpr = Math.min(window.devicePixelRatio || 1, 1.25) * quality.level;
    renderer.setPixelRatio(dpr);
    renderer.setSize(width, height, false);
    const w = Math.max(1, Math.floor(width * dpr));
    const h = Math.max(1, Math.floor(height * dpr));
    sceneTarget.setSize(w, h);
    bloomTargets[0].setSize(Math.max(1, w >> 2), Math.max(1, h >> 2));
    bloomTargets[1].setSize(Math.max(1, w >> 2), Math.max(1, h >> 2));
    bloomTargets[2].setSize(Math.max(1, w >> 3), Math.max(1, h >> 3));
    bloomTargets[3].setSize(Math.max(1, w >> 3), Math.max(1, h >> 3));
    auroraTarget.setSize(Math.max(1, w >> 1), Math.max(1, h >> 1));
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    setCamera(0, 0);
    const tanY = Math.tan(THREE.MathUtils.degToRad(CAMERA.fov / 2));
    shared.uHalfTan.value = tanY * camera.aspect * 1.12;
    placeField(shared.uHalfTan.value);
    skyUniforms.uTan.value.set(tanY * camera.aspect, tanY);
    skyUniforms.uAspect.value = camera.aspect;
    compositeMaterial.uniforms.uAspect.value = camera.aspect;
    const pixel = h / (2 * tanY);
    for (const uniforms of [speckUniforms, fieldFlyUniforms, swarmUniforms]) uniforms.uPixel.value = pixel;

    place = placement(root);
    placeStill();
    ndc.set(place.x / width * 2 - 1, -(place.y / height * 2 - 1));
    raycaster.setFromCamera(ndc, camera);
    raycaster.ray.intersectPlane(markPlane, anchor);
    // Perspective shrinks with depth along the view, not distance: the mark stands off to one side.
    const depth = -tmp.copy(anchor).applyMatrix4(camera.matrixWorldInverse).z;
    const unitsPerPixel = 2 * depth * tanY / height;
    scale = Math.max(0.05, place.size * unitsPerPixel / MARK.height);
    anchor.x -= MARK.center.x * scale;
    anchor.y -= MARK.center.y * scale;
    mark.scale.setScalar(scale);
    speckUniforms.uLocalScale.value = scale;
    skyUniforms.uGlowCenter.value.set(place.x / width, 1 - place.y / height);
    skyUniforms.uGlowRadius.value = place.size / height * 0.5;
    if (place.text) {
      const t = place.text;
      compositeMaterial.uniforms.uScrim.value.set(t.left / width, 1 - t.bottom / height, t.right / width, 1 - t.top / height);
      compositeMaterial.uniforms.uScrimStrength.value = 0.42;
    }
  }

  // The pointer: a lamp, a swarm of fireflies round it, and a hand that parts grass and beads.
  const pointer = new THREE.Vector2(0, 0);
  let pointerActive = false;
  let lastPointer = 0;
  let presence = 0;
  const lampTarget = new THREE.Vector3();
  const lampPosition = new THREE.Vector3(0, -50, 0);
  const groundPoint = new THREE.Vector3(0, -100, 0);
  const lookOffset = new THREE.Vector2();
  let groundPresence = 0;
  let kick = null;
  let gustAge = 99;
  let time = reduceMotion ? 7.3 : 0;
  function onPointer(event) {
    const rect = root.getBoundingClientRect();
    pointer.set((event.clientX - rect.left) / rect.width * 2 - 1, -((event.clientY - rect.top) / rect.height * 2 - 1));
    pointerActive = true;
    lastPointer = performance.now();
  }
  function onPress(event) {
    onPointer(event);
    raycaster.setFromCamera(pointer, camera);
    if (raycaster.ray.intersectPlane(groundPlane, tmp) && camera.position.distanceTo(tmp) < 60) shared.uGust.value.set(tmp.x, tmp.z, 0, 1);
    else shared.uGust.value.set(anchor.x, anchor.z - 3, 0, 1);
    gustAge = 0;
    kick = new THREE.Vector3((Math.random() - 0.5) * 30, 6 + Math.random() * 6, (Math.random() - 0.5) * 20);
  }
  function onLeave() {
    pointerActive = false;
  }
  const hero = root.closest('.dw-hero') || root;
  if (!reduceMotion) {
    hero.addEventListener('pointermove', onPointer, { passive: true });
    hero.addEventListener('pointerdown', onPress, { passive: true });
    hero.addEventListener('pointerleave', onLeave, { passive: true });
  }

  // Place the beads, cords and leaves where the chains are.
  const matrix = new THREE.Matrix4();
  const quaternion = new THREE.Quaternion();
  const size = new THREE.Vector3();
  const down = new THREE.Vector3(0, -1, 0);
  const upAxis = new THREE.Vector3(0, 1, 0);
  const direction = new THREE.Vector3();
  const midpoint = new THREE.Vector3();
  const twist = new THREE.Quaternion();
  function poseStrings() {
    let index = 0;
    for (const string of strings) {
      const nodes = string.points;
      for (let i = 1; i < nodes.length; i++) {
        const a = nodes[i - 1].p;
        const b = nodes[i].p;
        direction.subVectors(b, a);
        const length = direction.length();
        if (length > 1e-6) direction.divideScalar(length);
        else direction.copy(down);
        quaternion.setFromUnitVectors(upAxis, direction);
        midpoint.addVectors(a, b).multiplyScalar(0.5);
        size.set(0.0045, length, 0.0045);
        matrix.compose(midpoint, quaternion, size);
        cords.setMatrixAt(index, matrix);
        const kind = (i + string.nodes) % 3;
        if (i === nodes.length - 1) size.set(0.016, 0.016, 0.016);
        else if (kind === 0) size.set(0.04, 0.04, 0.04);
        else if (kind === 1) size.set(0.026, 0.046, 0.026);
        else size.set(0.03, 0.03, 0.03);
        matrix.compose(b, quaternion, size);
        beads.setMatrixAt(index, matrix);
        index++;
      }
      const end = nodes[nodes.length - 1].p;
      const before = nodes[nodes.length - 2].p;
      direction.subVectors(end, before);
      if (direction.lengthSq() < 1e-10) direction.copy(down);
      direction.normalize();
      quaternion.setFromUnitVectors(down, direction);
      twist.setFromAxisAngle(direction, string.twist);
      string.leafMesh.quaternion.multiplyQuaternions(twist, quaternion);
      string.leafMesh.position.copy(end);
    }
    beads.instanceMatrix.needsUpdate = true;
    cords.instanceMatrix.needsUpdate = true;
  }

  // The loop.
  const clock = new THREE.Clock();
  let visible = false;
  let running = false;
  let first = true;
  let lost = false;
  const gravity = new THREE.Vector3();
  const windLocal = new THREE.Vector3();
  const pointerLocal = new THREE.Vector3();
  const inverse = new THREE.Quaternion();
  let accumulator = 0;

  function simulate(dt) {
    inverse.copy(mark.quaternion).invert();
    gravity.set(0, -1, 0).applyQuaternion(inverse);
    const gust = 0.5 + 0.5 * Math.sin(time * 0.61) * Math.sin(time * 0.23 + 1.3);
    windLocal.set(WIND.x, 0, WIND.y * 0.6).applyQuaternion(inverse).multiplyScalar(0.6 + gust);
    markLocal.uWindLocal.value.copy(windLocal).multiplyScalar(0.5);
    const forces = { gravity, wind: windLocal, gust, pointer: pointerLocal, push: markLocal.uPushLocal.value.w, kick };
    accumulator += dt;
    let steps = 0;
    while (accumulator >= 1 / 120 && steps < 8) {
      stepStrings(strings, 1 / 120, time, forces);
      forces.kick = null;
      accumulator -= 1 / 120;
      steps++;
    }
    kick = null;
  }

  function frame() {
    running = false;
    if (lost) return;
    const rawDt = clock.getDelta();
    const dt = Math.min(rawDt, 0.05);
    if (!reduceMotion && rawDt < 0.5) {
      quality.slow = rawDt > 1 / 40 ? quality.slow + rawDt : Math.max(0, quality.slow - rawDt * 0.5);
      if (quality.slow > 1.5 && (quality.level > 0.5 || quality.blades > 0.35)) {
        quality.blades = Math.max(0.35, quality.blades - 0.2);
        quality.level = Math.max(0.5, quality.level - 0.15);
        field.instanceCount = Math.round(bladeBudget * quality.blades);
        quality.slow = 0;
        layout();
      }
    }
    if (!reduceMotion) time += dt;
    shared.uTime.value = time;
    gustAge += dt;
    shared.uGust.value.z = gustAge;

    // The camera drifts a little, and follows the pointer a little more.
    const idle = !pointerActive || performance.now() - lastPointer > 4000;
    const drift = reduceMotion ? 0 : 1;
    const ease = reduceMotion ? 1 : 1 - Math.exp(-dt * (idle ? 0.45 : 1.6));
    lookOffset.x += ((idle ? 0 : pointer.x * 0.12) - lookOffset.x) * ease;
    lookOffset.y += ((idle ? 0 : pointer.y * 0.05) - lookOffset.y) * ease;
    setCamera(Math.sin(time * 0.05) * 0.25 * drift + lookOffset.x, Math.sin(time * 0.07) * 0.05 * drift + lookOffset.y);
    const forward = camera.getWorldDirection(tmp);
    skyUniforms.uForward.value.copy(forward);
    skyUniforms.uRight.value.set(1, 0, 0).applyQuaternion(camera.quaternion);
    skyUniforms.uUp.value.set(0, 1, 0).applyQuaternion(camera.quaternion);

    // The lamp: the pointer while it moves over the hero, a slow drift round the mark otherwise.
    if (idle) {
      lampTarget.set(anchor.x + Math.sin(time * 0.4) * 0.9 * scale, anchor.y + 0.3 * scale + Math.cos(time * 0.29) * 0.6 * scale, anchor.z + 1.1 * scale);
    } else {
      raycaster.setFromCamera(pointer, camera);
      const lampPlane = new THREE.Plane(new THREE.Vector3(0, 0, 1), -(anchor.z + 1.0 * scale));
      if (raycaster.ray.intersectPlane(lampPlane, tmp)) lampTarget.copy(tmp);
    }
    const wanted = idle ? 0 : 1;
    presence += (wanted - presence) * (reduceMotion ? 1 : Math.min(1, dt * (idle ? 1.2 : 3)));
    lampPosition.lerp(lampTarget, reduceMotion ? 1 : Math.min(1, dt * 6));
    lamp.position.copy(lampPosition);
    lamp.intensity = presence * 5 * scale * scale;
    shared.uLampPos.value.copy(lampPosition);
    shared.uLamp.value = presence * 0.8;
    swarmUniforms.uSwarm.value.copy(lampPosition);
    swarmUniforms.uSwarmPresence.value = presence;

    // The pointer over the field parts the grass there.
    let overGround = false;
    if (!idle) {
      raycaster.setFromCamera(pointer, camera);
      if (raycaster.ray.intersectPlane(groundPlane, tmp) && camera.position.distanceTo(tmp) < 45) {
        groundPoint.lerp(tmp, groundPresence > 0.05 ? Math.min(1, dt * 8) : 1);
        overGround = true;
      }
    }
    groundPresence += ((overGround ? 1 : 0) - groundPresence) * Math.min(1, dt * 4);
    fieldUniforms.uPush.value.set(groundPoint.x, groundPoint.z, groundPresence, 1.2 + camera.position.distanceTo(groundPoint) * 0.05);

    // The mark faces the camera squarely, wherever it stands in the view, and only rocks a little.
    mark.position.set(anchor.x, anchor.y + Math.sin(time * 0.6) * 0.02 * scale, anchor.z);
    tmp.subVectors(camera.position, mark.position);
    const yaw = Math.atan2(tmp.x, tmp.z);
    const pitch = -Math.asin(THREE.MathUtils.clamp(tmp.y / Math.max(tmp.length(), 1e-6), -1, 1));
    mark.rotation.set(pitch, yaw, Math.sin(time * 0.27) * 0.025, 'YXZ');
    mark.updateMatrixWorld();

    // The hollow's glow flickers with its specks.
    glow.position.set(0.6, 0.1, 0.35);
    mark.localToWorld(glow.position);
    const flicker = 0.8 + 0.2 * Math.sin(time * 3.1) * Math.sin(time * 1.7 + 0.4);
    glow.intensity = 1.6 * flicker * scale * scale;
    shared.uGlowPos.value.copy(glow.position);
    shared.uGlow.value = 0.9 * flicker;
    key.position.copy(anchor).addScaledVector(shared.uKeyDir.value, 10);
    key.target.position.copy(anchor);
    rim.position.copy(anchor).addScaledVector(shared.uMoonDir.value, 10);
    rim.target.position.copy(anchor);

    // The pointer's reach in the mark's space, for the beads and the fur.
    if (!idle) {
      raycaster.setFromCamera(pointer, camera);
      if (raycaster.ray.intersectPlane(markPlane.set(new THREE.Vector3(0, 0, 1), -anchor.z), tmp)) {
        pointerLocal.copy(mark.worldToLocal(tmp.clone()));
        markLocal.uPushLocal.value.set(pointerLocal.x, pointerLocal.y, 0.1, presence);
      }
    } else markLocal.uPushLocal.value.w *= 0.9;

    simulate(reduceMotion ? 0 : dt);
    poseStrings();
    scene.updateMatrixWorld();

    pass(auroraMaterial, auroraTarget);
    renderer.setRenderTarget(sceneTarget);
    renderer.setClearColor(0x000000, 1);
    renderer.clear();
    renderer.render(scene, camera);

    pass(brightMaterial, bloomTargets[0]);
    blur(bloomTargets[0], bloomTargets[1], 1.0);
    blur(bloomTargets[0], bloomTargets[1], 2.0);
    copyMaterial.uniforms.tInput.value = bloomTargets[0].texture;
    pass(copyMaterial, bloomTargets[2]);
    blur(bloomTargets[2], bloomTargets[3], 1.5);
    blur(bloomTargets[2], bloomTargets[3], 3.0);
    pass(compositeMaterial, null);

    if (first) {
      first = false;
      root.classList.add('is-live');
    }
    if (visible && !reduceMotion && !document.hidden) requestFrame();
  }

  function requestFrame() {
    if (running || lost) return;
    running = true;
    requestAnimationFrame(frame);
  }

  canvas.addEventListener('webglcontextlost', (event) => {
    event.preventDefault();
    lost = true;
    root.classList.remove('is-live');
  });
  canvas.addEventListener('webglcontextrestored', () => {
    canvas.remove();
    still.remove();
    root.classList.remove('is-live', 'is-placed');
    mount(root);
  });

  layout();
  // Let the strings settle before the first frame, so they hang rather than drop.
  mark.position.copy(anchor);
  mark.updateMatrixWorld();
  for (let i = 0; i < 240; i++) {
    inverse.copy(mark.quaternion).invert();
    gravity.set(0, -1, 0).applyQuaternion(inverse);
    stepStrings(strings, 1 / 120, time - 2 + i / 120, { gravity, wind: windLocal.set(WIND.x * 0.6, 0, WIND.y * 0.3), gust: 0.4, pointer: pointerLocal, push: 0, kick: null });
  }
  // Compile every shader before the first frame, in parallel where the browser can, while the still
  // stands in; then start drawing whenever the hero is on screen.
  const warm = new THREE.Scene();
  for (const material of [auroraMaterial, brightMaterial, blurMaterial, copyMaterial, compositeMaterial]) {
    const mesh = new THREE.Mesh(quad, material);
    mesh.frustumCulled = false;
    warm.add(mesh);
  }
  const compiled = renderer.compileAsync
    ? Promise.all([renderer.compileAsync(scene, camera), renderer.compileAsync(warm, postCamera)])
    : Promise.resolve();
  compiled.catch(() => {}).then(() => {
    new ResizeObserver(() => {
      layout();
      requestFrame();
    }).observe(root);
    if (document.fonts) {
      document.fonts.ready.then(() => {
        layout();
        requestFrame();
      });
    }
    new IntersectionObserver((entries) => {
      visible = entries.some((entry) => entry.isIntersecting);
      if (visible) {
        clock.getDelta();
        requestFrame();
      }
    }).observe(root);
    document.addEventListener('visibilitychange', () => {
      if (!document.hidden && visible) {
        clock.getDelta();
        requestFrame();
      }
    });
  });
}

for (const root of document.querySelectorAll('[data-dw-hero-art]')) mount(root);
