
<template>
    <div>
        <VuePlotly :data="data" :layout="layout" :display-mode-bar="true"></VuePlotly>
    </div>
</template>

<script lang="ts">
import { VuePlotly } from 'vue3-plotly'
import map_editor from "./map_editor.vue"
export default {
    components: {
    VuePlotly
  },
  data() {
    return {
      data:[{
        x: map_editor.get_x_axis(),
        y: map_editor.get_y_axis(),
        z: map_editor.get_z_axis(),
        type:"surface"
      }],
      layout:{
        dragmode: false,
        displayModeBar: false,
        displayToolBar: false,
        title: 'TEST',
        autosize: true,
        //width: this.props.size.width,
        //height: this.props.size.width,
        margin: {
            l: 0,
            r: 0,
            b: 0,
            t: 0,
        },
        xaxis: {
          title: {
            text: map_editor.get_x_axis_name(),
            ticksuffix: "mBar"
          }
        },
        yaxis: {
          title: {
            text: map_editor.get_y_axis_name(),
            ticksuffix: "C"
          }
        },
        zaxis: {
          title: {
            text: map_editor.get_z_axis_name(),
            ticksuffix: "mA"
          }
        },
      },
      config: {
        responsive: true
      }
    }
  }
}
</script>