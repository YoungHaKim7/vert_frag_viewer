#version 450
out vec4 vertexColor; // specify a color output to the fragment shader

void main(int vertexID: SV_VertexID)
{
    // The viewer supplies no vertex buffer: position the triangle from the
    // vertex index (SV_VertexID / gl_VertexID).
    vec2 positions[3] = vec2[](
        vec2(-0.5, -0.5),
        vec2( 0.5, -0.5),
        vec2( 0.0,  0.5)
    );

    gl_Position = vec4(positions[vertexID], 0.0, 1.0);
    vertexColor = vec4(0.5, 0.0, 0.0, 1.0); // set the output variable to a dark-red color
}
