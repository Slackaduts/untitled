<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.1" name="ADWinter_TileD" tilewidth="48" tileheight="48" tilecount="256" columns="16">
 <image source="ADWinter_TileD.png" width="768" height="768"/>
 <tile id="121" type="TileLight">
  <properties>
   <property name="color_b" type="float" value="0.3"/>
   <property name="color_g" type="float" value="0.7"/>
   <property name="color_r" type="float" value="1"/>
   <property name="flicker" type="bool" value="false"/>
   <property name="intensity" type="float" value="1.5"/>
   <property name="pulse" type="bool" value="true"/>
   <property name="radius" type="int" value="120"/>
  </properties>
 </tile>
 <tile id="168">
  <objectgroup draworder="index" id="2">
   <object id="1" x="1" y="-0.818182" width="33.1818" height="46.4318"/>
  </objectgroup>
 </tile>
 <tile id="170" type="TileBillboard">
  <properties>
   <property name="collider_depth" type="float" value="30"/>
   <property name="origin_x" type="float" value="0.654"/>
   <property name="origin_y" type="float" value="0.783"/>
  </properties>
  <objectgroup draworder="index" id="2">
   <object id="1" x="15.2727" y="-0.181818" width="32.7273" height="46.9091"/>
  </objectgroup>
 </tile>
 <tile id="179" type="TileBillboard"/>
 <tile id="220" type="TileBillboard"/>
 <tile id="228" type="TileBillboard"/>
 <tile id="235" type="TileBillboard"/>
 <tile id="236" type="TileBillboard"/>
 <tile id="237" type="TileBillboard"/>
</tileset>
