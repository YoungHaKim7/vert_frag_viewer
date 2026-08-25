vec3 color0 = vec3(0.5, 1.0, 0.5);
vec3 color1 = vec3(1.0, 0.5, 0.5);

vec3 circle(vec2 uv, vec2 center, float radius) {
    float dist = distance(uv, center);
    float aa = fwidth(dist);
    float inside = smoothstep(radius - aa, radius + aa, dist);
    return mix(color0, color1, inside);
}

void mainImage( out vec4 fragColor, in vec2 fragCoord )
{
    vec2 uv = fragCoord/iResolution.xy;
    uv.x *= iResolution.x / iResolution.y;
    float radius = 0.15 * (1.0 + sin(iTime * 2.0)) + 0.2;
    vec3 color = circle(uv, vec2(0.5), radius);
    fragColor = vec4(color,1.0);
}
