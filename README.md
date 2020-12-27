# Easel
A Shader playground for creating digital paintings.

## Motivation
Like many others,  I enjoy creative coding with shaders. Websites like Shadertoy are excellent for prototyping and learning from others. However, Shadertoy's usability can vary greatly based on geographic location and the website sometimes glitches on certain browsers. Performance is generally poor and complex shaders often crash my browser. In addition, once I have created a work, I often want to save it to file and print it for display. As far as I can tell, the current tools created by the community lack the ability to export high-res images suitable for editing and printing workflows. Screenshotting can only go so far.

Additionally, like many others, I wanted to pick up a new skill during all the down time caused by the 2020 pandemic. I started experimenting with Rust and quickly fell in love with the language.

## Usage
Easel is designed to be familiar to anyone who has used Shadertoy. Bring your fragment shader and it does the rest.

There are many configuration options available. Use `easel --help` to see the list.

## Creating Digital Paintings
When rendering a painting for writing to disk, Easel uses high quality 16-bit textures and writes the data to uncompressed 16-bit TIFF files. 

## Documentation
Detailed documentation is available on docs.rs.

## Building Easel
Internally, Easel uses the [shaderc](https://github.com/google/shaderc-rs) crate, which in turn uses a C++ library. This requires [CMake](https://cmake.org), [Python](https://python.org) and [Ninja](https://ninja-build.org/) to be installed. 

Once you have all the dependencies installed, simply build with Cargo. I recommend building in Release mode for performance.python