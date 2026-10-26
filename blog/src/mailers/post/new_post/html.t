{#- Mail templates are `.t` files: Tera does NOT autoescape them (only
    .html/.htm/.xml). Every interpolated value must be escaped explicitly. -#}
{% extends "base.t" %}
{% block title %}{{ title | escape }}{% endblock %}
{% block body %}
<h1>{{ title | escape }}</h1>
{% if reading_minutes %}<p><em>{{ reading_minutes }} min read</em></p>{% endif %}
<p>{{ excerpt | escape }}</p>
<p><a href="{{ url | escape }}">Read the post &rarr;</a></p>
{% endblock %}
{% block footer %}
You get this because you subscribed to Loco Blog.
<a href="{{ unsubscribe_url | escape }}">Unsubscribe</a>.
{% endblock %}
